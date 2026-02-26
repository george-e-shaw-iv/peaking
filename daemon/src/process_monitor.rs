use std::sync::Arc;
use sysinfo::{ProcessesToUpdate, System};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};

use crate::config::Config;
use crate::event::DaemonEvent;

const POLL_INTERVAL_SECS: u64 = 2;

/// Returns `true` if `active_exe` appears in `process_names` (case-insensitive).
/// Mirrors the still-running check used in the monitor loop.
#[cfg(test)]
fn exe_is_running(active_exe: &str, process_names: &[&str]) -> bool {
    let target = active_exe.to_lowercase();
    process_names.iter().any(|n| n.to_lowercase() == target)
}

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

        // Detect if the active game has exited — check explicitly so that
        // ProcessStopped is sent even when another configured game is running.
        if let Some(exe) = &active_exe {
            let target = exe.to_lowercase();
            let still_running = sys
                .processes()
                .values()
                .any(|p| p.name().to_string_lossy().to_lowercase() == target);
            if !still_running {
                eprintln!("[monitor] Exited: {exe}");
                active_exe = None;
                if tx.send(DaemonEvent::ProcessStopped).await.is_err() {
                    break;
                }
            }
        }

        // Start recording the first matching game if none is active.
        if active_exe.is_none() {
            if let Some(app) = found {
                eprintln!("[monitor] Detected: {}", app.display_name);
                active_exe = Some(app.executable_name.clone());
                if tx.send(DaemonEvent::ProcessStarted(app)).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::exe_is_running;

    #[test]
    fn exe_found_in_exact_match() {
        assert!(exe_is_running("game.exe", &["game.exe"]));
    }

    #[test]
    fn exe_found_case_insensitive_active_upper() {
        assert!(exe_is_running("GAME.EXE", &["game.exe"]));
    }

    #[test]
    fn exe_found_case_insensitive_process_upper() {
        assert!(exe_is_running("game.exe", &["Game.exe", "OTHER.EXE"]));
    }

    #[test]
    fn exe_not_found_when_absent() {
        assert!(!exe_is_running("game.exe", &["other.exe", "notgame.exe"]));
    }

    #[test]
    fn exe_not_found_in_empty_list() {
        assert!(!exe_is_running("game.exe", &[]));
    }

    /// Regression: active game exited but a different configured game is still
    /// running.  The still-running check must look up the ACTIVE exe specifically,
    /// not just any configured exe.
    #[test]
    fn active_game_gone_other_game_present() {
        // game_a.exe has exited; only game_b.exe is in the process list.
        let running = ["game_b.exe"];
        assert!(!exe_is_running("game_a.exe", &running));
        assert!(exe_is_running("game_b.exe", &running));
    }

    #[test]
    fn active_game_still_running_alongside_other() {
        let running = ["game_a.exe", "game_b.exe"];
        assert!(exe_is_running("game_a.exe", &running));
    }
}
