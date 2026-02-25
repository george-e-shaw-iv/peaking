mod audio_capture;
mod capture;
mod config;
mod encoder;
mod event;
mod flush;
mod hotkey;
mod paths;
mod pipeline;
mod process_monitor;
mod ring_buffer;
mod status;

use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, RwLock};

use crate::config::DEFAULT_BUFFER_LENGTH_SECS;
use crate::ring_buffer::RingBuffer;

#[tokio::main]
async fn main() {
    // ── App data directory ────────────────────────────────────────────────────
    let app_dir = paths::app_data_dir();
    if let Err(e) = std::fs::create_dir_all(&app_dir) {
        eprintln!("Failed to create app data directory {}: {e}", app_dir.display());
        std::process::exit(1);
    }

    // ── Configuration ─────────────────────────────────────────────────────────
    let config_path = paths::config_file_path();
    let initial_config = config::load_or_default(&config_path).unwrap_or_else(|e| {
        eprintln!("[config] Error (using defaults): {e}");
        config::Config::default()
    });
    let initial_hotkey = initial_config.global.hotkey.clone();
    let shared_config = Arc::new(RwLock::new(initial_config));

    // ── Initial status ────────────────────────────────────────────────────────
    let status_path = paths::status_file_path();
    let mut current_status = status::DaemonStatus::new();
    status::write_status(&status_path, &current_status);

    // ── Ring buffer ───────────────────────────────────────────────────────────
    let ring_buffer: Arc<Mutex<RingBuffer>> =
        Arc::new(Mutex::new(RingBuffer::new(DEFAULT_BUFFER_LENGTH_SECS)));

    let (event_tx, mut event_rx) = mpsc::channel::<event::DaemonEvent>(32);

    // ── Background tasks ──────────────────────────────────────────────────────
    tokio::spawn(config::watch_config(config_path, event_tx.clone()));
    tokio::spawn(process_monitor::run(Arc::clone(&shared_config), event_tx.clone()));

    let hotkey_handle = hotkey::start(&initial_hotkey, event_tx.clone());

    // Graceful shutdown on Ctrl+C.
    {
        let tx = event_tx.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                let _ = tx.send(event::DaemonEvent::Shutdown).await;
            }
        });
    }

    println!("peaking-daemon v{} started", env!("CARGO_PKG_VERSION"));

    // ── Event loop ────────────────────────────────────────────────────────────
    let mut active_pipeline: Option<pipeline::Pipeline> = None;
    // Tracks the currently-recording app so we can apply its hotkey/buffer overrides.
    let mut active_app: Option<config::ApplicationConfig> = None;

    while let Some(evt) = event_rx.recv().await {
        match evt {
            event::DaemonEvent::ProcessStarted(app) => {
                if let Some(p) = active_pipeline.take() {
                    p.stop().await;
                }

                println!("Recording started: {}", app.display_name);
                current_status.state = status::DaemonState::Recording;
                current_status.active_application = Some(app.display_name.clone());
                current_status.error = None;
                status::write_status(&status_path, &current_status);

                let cfg = shared_config.read().await;
                {
                    let mut rb = ring_buffer.lock().unwrap();
                    rb.clear();
                    rb.resize(app.effective_buffer_length(&cfg.global));
                }
                hotkey_handle.update_key(app.effective_hotkey(&cfg.global));
                active_pipeline = Some(pipeline::Pipeline::start(
                    &app,
                    &cfg,
                    Arc::clone(&ring_buffer),
                ));
                active_app = Some(app);
            }

            event::DaemonEvent::ProcessStopped => {
                if let Some(p) = active_pipeline.take() {
                    p.stop().await;
                }
                active_app = None;

                // Restore the global hotkey now that no per-app override is active.
                let global_hotkey = shared_config.read().await.global.hotkey.clone();
                hotkey_handle.update_key(&global_hotkey);

                println!("Recording stopped");
                current_status.state = status::DaemonState::Idle;
                current_status.active_application = None;
                status::write_status(&status_path, &current_status);
            }

            event::DaemonEvent::ConfigReloaded(new_config) => {
                println!("Config reloaded");
                // Apply per-app overrides if a game is currently being recorded.
                let effective_key = match &active_app {
                    Some(app) => app.effective_hotkey(&new_config.global).to_string(),
                    None => new_config.global.hotkey.clone(),
                };
                hotkey_handle.update_key(&effective_key);
                {
                    let new_capacity = match &active_app {
                        Some(app) => app.effective_buffer_length(&new_config.global),
                        None => new_config.global.buffer_length_secs,
                    };
                    let mut rb = ring_buffer.lock().unwrap();
                    rb.resize(new_capacity);
                }
                *shared_config.write().await = new_config;
            }

            event::DaemonEvent::FlushRequested => {
                if active_pipeline.is_none() {
                    // No active recording — silently no-op (task 8.4).
                    continue;
                }

                let display_name = match &current_status.active_application {
                    Some(name) => name.clone(),
                    None => {
                        eprintln!("[flush] FlushRequested but active_application is unset");
                        continue;
                    }
                };

                // Snapshot the ring buffer without draining it so recording
                // continues to accumulate while the MP4 is being written.
                let (segments, video_params, audio_params) = {
                    let rb = ring_buffer.lock().unwrap();
                    let segs = rb.segments().iter().cloned().collect::<Vec<_>>();
                    let vp = rb.video_params.clone();
                    let ap = rb.audio_params.clone();
                    (segs, vp, ap)
                };

                let (video_params, audio_params) = match (video_params, audio_params) {
                    (Some(v), Some(a)) => (v, a),
                    _ => {
                        eprintln!("[flush] Codec parameters not yet available; skipping flush");
                        continue;
                    }
                };

                let clip_output_dir = {
                    let cfg = shared_config.read().await;
                    cfg.global.clip_output_dir.clone()
                };

                // Signal flushing state to the GUI.
                current_status.state = status::DaemonState::Flushing;
                status::write_status(&status_path, &current_status);

                println!("[flush] Saving clip for '{display_name}' ({} segments)…", segments.len());

                match flush::flush_to_disk(
                    segments,
                    video_params,
                    audio_params,
                    clip_output_dir,
                    display_name,
                )
                .await
                {
                    Ok(path) => {
                        let timestamp = chrono::Local::now().to_rfc3339();
                        println!("[flush] Clip saved: {}", path.display());
                        current_status.last_clip_path =
                            Some(path.to_string_lossy().into_owned());
                        current_status.last_clip_timestamp = Some(timestamp);
                        current_status.error = None;
                    }
                    Err(e) => {
                        eprintln!("[flush] Failed to save clip: {e}");
                        current_status.error = Some(format!("Flush failed: {e}"));
                    }
                }

                // Return to recording state regardless of flush outcome.
                current_status.state = status::DaemonState::Recording;
                status::write_status(&status_path, &current_status);
            }

            event::DaemonEvent::Shutdown => {
                println!("Shutting down");
                if let Some(p) = active_pipeline.take() {
                    p.stop().await;
                }
                current_status.state = status::DaemonState::Idle;
                current_status.active_application = None;
                current_status.error = None;
                status::write_status(&status_path, &current_status);
                break;
            }
        }
    }

    hotkey_handle.stop();
}
