/// Capture + encode + ring-buffer pipeline for a single recording session.
///
/// A `Pipeline` is started when a watched game process is detected and stopped
/// when that process exits.  It owns:
///   - a screen-capture task (WGC, Phase 4)
///   - an audio-capture task (WASAPI, Phase 5)
///   - an encoder task (NVENC H.264 + AAC, Phase 6)
///
/// The ring buffer (Phase 7) is shared via `Arc<Mutex<RingBuffer>>` so that
/// the hotkey handler (Phase 8) can drain it for flushing (Phase 9).
use std::sync::{Arc, Mutex};

use tokio::{sync::{mpsc, watch}, task::JoinHandle};

use crate::audio_capture::{self, RawAudio};
use crate::capture::{self, RawFrame};
use crate::config::{ApplicationConfig, Config};
use crate::encoder::{EncoderConfig, SegmentEncoder};
use crate::ring_buffer::RingBuffer;

/// A running capture + encode pipeline.
pub struct Pipeline {
    /// Setting this to `true` signals all sub-tasks to stop.
    stop_tx: watch::Sender<bool>,
    handles: Vec<JoinHandle<()>>,
}

impl Pipeline {
    /// Starts the pipeline for `app` using `config`.
    /// The encoder feeds completed segments into `ring_buffer`.
    pub fn start(
        app: &ApplicationConfig,
        config: &Config,
        ring_buffer: Arc<Mutex<RingBuffer>>,
    ) -> Self {
        let encoder_config = EncoderConfig {
            // Resolution will be filled in by the first captured frame; use
            // defaults here — the encoder will be re-initialised if the
            // resolution changes (this is a future enhancement).
            width: 1920,
            height: 1080,
            fps: 60,
            sample_rate: 48_000,
            channels: 2,
            video_bitrate: 8_000_000,
            audio_bitrate: 192_000,
        };

        let (stop_tx, stop_rx) = watch::channel(false);
        let (frame_tx, frame_rx) = mpsc::channel::<RawFrame>(8);
        let (audio_tx, audio_rx) = mpsc::channel::<RawAudio>(32);

        let mut handles = vec![];

        // ── Screen capture task ───────────────────────────────────────────────
        {
            let stop_rx = stop_rx.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = capture::run(frame_tx, stop_rx).await {
                    eprintln!("[capture] Stopped: {e}");
                }
            }));
        }

        // ── Audio capture task ────────────────────────────────────────────────
        {
            let stop_rx = stop_rx.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = audio_capture::run(audio_tx, stop_rx).await {
                    eprintln!("[audio] Stopped: {e}");
                }
            }));
        }

        // ── Encoder task ──────────────────────────────────────────────────────
        {
            let ring_buffer = Arc::clone(&ring_buffer);
            let display_name = app.display_name.clone();
            let effective_buffer_secs = app.effective_buffer_length(&config.global);

            handles.push(tokio::spawn(async move {
                run_encoder(
                    frame_rx,
                    audio_rx,
                    ring_buffer,
                    encoder_config,
                    effective_buffer_secs,
                    &display_name,
                )
                .await;
            }));
        }

        Pipeline { stop_tx, handles }
    }

    /// Signals all sub-tasks to stop and waits for them to finish.
    pub async fn stop(self) {
        let _ = self.stop_tx.send(true);
        for handle in self.handles {
            let _ = handle.await;
        }
    }
}

/// Encoder loop: receives raw frames and audio, encodes them, and pushes
/// completed [`EncodedSegment`]s into the ring buffer.
async fn run_encoder(
    mut frame_rx: mpsc::Receiver<RawFrame>,
    mut audio_rx: mpsc::Receiver<RawAudio>,
    ring_buffer: Arc<Mutex<RingBuffer>>,
    config: EncoderConfig,
    buffer_secs: u32,
    display_name: &str,
) {
    let mut encoder = match SegmentEncoder::new(&config) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("[encoder] Init failed for '{display_name}': {err}");
            return;
        }
    };

    // Initialise ring buffer capacity and store codec parameters.
    {
        let mut rb = ring_buffer.lock().unwrap();
        rb.resize(buffer_secs);
        rb.video_params = Some(encoder.video_params.clone());
        rb.audio_params = Some(encoder.audio_params.clone());
    }

    eprintln!("[encoder] Started for '{display_name}' ({buffer_secs}s buffer)");

    loop {
        tokio::select! {
            frame = frame_rx.recv() => {
                let Some(frame) = frame else { break };
                match encoder.push_video_frame(&frame) {
                    Ok(Some(segment)) => {
                        ring_buffer.lock().unwrap().push(segment);
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("[encoder] Video error: {e}"),
                }
            }
            audio = audio_rx.recv() => {
                let Some(audio) = audio else { break };
                if let Err(e) = encoder.push_audio(&audio) {
                    eprintln!("[encoder] Audio error: {e}");
                }
            }
            else => break,
        }
    }

    // Flush any remaining buffered data.
    if let Ok(Some(segment)) = encoder.flush() {
        ring_buffer.lock().unwrap().push(segment);
    }

    eprintln!("[encoder] Stopped for '{display_name}'");
}
