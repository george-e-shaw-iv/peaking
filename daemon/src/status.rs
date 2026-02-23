use serde::{Deserialize, Serialize};
use std::path::Path;

/// Current operational state of the daemon.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum DaemonState {
    /// No watched process is running; the ring buffer is inactive.
    Idle,
    /// A watched process is running and frames are being captured into the ring buffer.
    Recording,
    /// The ring buffer is currently being flushed and muxed to an MP4 file on disk.
    Flushing,
}

/// Runtime status written by the daemon to %APPDATA%\Peaking\status.toml.
/// The GUI reads this file (read-only) to display daemon state.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DaemonStatus {
    /// Daemon binary version (set from Cargo.toml at compile time).
    pub version: String,
    /// Current operational state.
    pub state: DaemonState,
    /// Display name of the application currently being recorded, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_application: Option<String>,
    /// Absolute path of the most recently saved clip, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_clip_path: Option<String>,
    /// RFC 3339 timestamp of the most recently saved clip, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_clip_timestamp: Option<String>,
    /// Human-readable error message if the daemon encountered a non-fatal error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DaemonStatus {
    /// Constructs the initial idle status on daemon startup.
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            state: DaemonState::Idle,
            active_application: None,
            last_clip_path: None,
            last_clip_timestamp: None,
            error: None,
        }
    }
}

/// Serializes `status` to TOML and writes it to `path`.
/// Creates the parent directory if it does not exist.
/// Logs errors to stderr rather than panicking — a status write failure should
/// never crash the daemon.
pub fn write_status(path: &Path, status: &DaemonStatus) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("[status] Failed to create directory {}: {e}", parent.display());
            return;
        }
    }
    match toml::to_string_pretty(status) {
        Ok(content) => {
            if let Err(e) = std::fs::write(path, content) {
                eprintln!("[status] Failed to write status file: {e}");
            }
        }
        Err(e) => eprintln!("[status] Failed to serialize status: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DaemonStatus::new ─────────────────────────────────────────────────────

    #[test]
    fn new_starts_idle() {
        let s = DaemonStatus::new();
        assert_eq!(s.state, DaemonState::Idle);
    }

    #[test]
    fn new_has_no_optional_fields() {
        let s = DaemonStatus::new();
        assert!(s.active_application.is_none());
        assert!(s.last_clip_path.is_none());
        assert!(s.last_clip_timestamp.is_none());
        assert!(s.error.is_none());
    }

    #[test]
    fn new_version_matches_cargo_pkg() {
        let s = DaemonStatus::new();
        assert_eq!(s.version, env!("CARGO_PKG_VERSION"));
    }

    // ── DaemonState serialization ─────────────────────────────────────────────

    #[test]
    fn state_serializes_to_lowercase() {
        // TOML requires a root table, so verify the value via DaemonStatus.
        let mut s = DaemonStatus::new();
        let idle = toml::to_string_pretty(&s).unwrap();
        assert!(idle.contains("state = \"idle\""));

        s.state = DaemonState::Recording;
        let recording = toml::to_string_pretty(&s).unwrap();
        assert!(recording.contains("state = \"recording\""));

        s.state = DaemonState::Flushing;
        let flushing = toml::to_string_pretty(&s).unwrap();
        assert!(flushing.contains("state = \"flushing\""));
    }

    #[test]
    fn state_round_trips_through_toml() {
        for state in [DaemonState::Idle, DaemonState::Recording, DaemonState::Flushing] {
            let mut status = DaemonStatus::new();
            status.state = state.clone();
            let serialized = toml::to_string_pretty(&status).unwrap();
            let deserialized: DaemonStatus = toml::from_str(&serialized).unwrap();
            assert_eq!(deserialized.state, state);
        }
    }

    // ── write_status ──────────────────────────────────────────────────────────

    #[test]
    fn write_status_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.toml");
        let status = DaemonStatus::new();
        write_status(&path, &status);
        assert!(path.exists());
    }

    #[test]
    fn write_status_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("status.toml");
        let status = DaemonStatus::new();
        write_status(&path, &status);
        assert!(path.exists());
    }

    #[test]
    fn write_status_content_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.toml");

        let mut original = DaemonStatus::new();
        original.state = DaemonState::Recording;
        original.active_application = Some("Rocket League".to_string());

        write_status(&path, &original);

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: DaemonStatus = toml::from_str(&content).unwrap();

        assert_eq!(parsed.state, DaemonState::Recording);
        assert_eq!(parsed.active_application.as_deref(), Some("Rocket League"));
    }

    #[test]
    fn write_status_omits_none_optional_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.toml");
        let status = DaemonStatus::new();
        write_status(&path, &status);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("active_application"));
        assert!(!content.contains("last_clip_path"));
        assert!(!content.contains("last_clip_timestamp"));
        assert!(!content.contains("error"));
    }

    #[test]
    fn write_status_includes_populated_optional_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.toml");

        let mut status = DaemonStatus::new();
        status.active_application = Some("Fortnite".to_string());
        status.last_clip_path = Some(r"C:\Clips\clip.mp4".to_string());
        status.error = Some("encoder failed".to_string());

        write_status(&path, &status);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("active_application"));
        assert!(content.contains("last_clip_path"));
        assert!(content.contains("error"));
    }
}
