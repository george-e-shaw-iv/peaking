use std::sync::Arc;
use sysinfo::{ProcessesToUpdate, System};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};

use crate::config::Config;
use crate::event::DaemonEvent;

const POLL_INTERVAL_SECS: u64 = 2;

/// Polls the OS process list every [`POLL_INTERVAL_SECS`] seconds and emits
/// [`DaemonEvent::ProcessStarted`] / [`DaemonEvent::ProcessStopped`] events
/// whenever a configured game executable appears or disappears.
///
/// Only one application is considered "active" at a time. If multiple watched
/// executables are running simultaneously, the first match in the config list wins.
pub async fn run(config: Arc<RwLock<Config>>, tx: mpsc::Sender<DaemonEvent>) {
    let mut sys = System::new();
    let mut active_exe: Option<String> = None;
    let mut ticker = interval(Duration::from_secs(POLL_INTERVAL_SECS));

    loop {
        ticker.tick().await;

        sys.refresh_processes(ProcessesToUpdate::All, false);

        let config = config.read().await;
        let found = config
            .applications
            .iter()
            .find(|app| {
                let target = app.executable_name.to_lowercase();
                sys.processes()
                    .values()
                    .any(|p| p.name().to_string_lossy().to_lowercase() == target)
            })
            .cloned();

        // Release the read lock before any awaits below.
        drop(config);

        match (active_exe.as_ref(), found) {
            (None, Some(app)) => {
                eprintln!("[monitor] Detected: {}", app.display_name);
                active_exe = Some(app.executable_name.clone());
                if tx.send(DaemonEvent::ProcessStarted(app)).await.is_err() {
                    break;
                }
            }
            (Some(exe), None) => {
                eprintln!("[monitor] Exited: {exe}");
                active_exe = None;
                if tx.send(DaemonEvent::ProcessStopped).await.is_err() {
                    break;
                }
            }
            _ => {} // No change.
        }
    }
}
