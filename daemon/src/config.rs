use anyhow::{Context, Result};
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

use crate::event::DaemonEvent;

pub const MIN_BUFFER_LENGTH_SECS: u32 = 5;
pub const MAX_BUFFER_LENGTH_SECS: u32 = 120;
pub const DEFAULT_BUFFER_LENGTH_SECS: u32 = 15;
pub const DEFAULT_HOTKEY: &str = "F8";
/// Resolved at runtime by expanding %USERPROFILE%.
pub const DEFAULT_CLIP_OUTPUT_DIR: &str = r"%USERPROFILE%\Videos\Peaking";

/// Root configuration structure. Deserialized from %APPDATA%\Peaking\config.toml.
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub global: GlobalConfig,
    #[serde(default)]
    pub applications: Vec<ApplicationConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            global: GlobalConfig::default(),
            applications: Vec::new(),
        }
    }
}

/// Global defaults applied when no per-application override exists.
#[derive(Debug, Deserialize)]
pub struct GlobalConfig {
    /// Length of the rolling video buffer in seconds. Clamped to [5, 120].
    #[serde(default = "default_buffer_length")]
    pub buffer_length_secs: u32,
    /// Virtual-key name of the clip hotkey (e.g. "F8").
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Directory under which per-game clip subdirectories are created.
    /// %USERPROFILE% is expanded at runtime.
    #[serde(default = "default_clip_output_dir")]
    pub clip_output_dir: String,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            buffer_length_secs: DEFAULT_BUFFER_LENGTH_SECS,
            hotkey: DEFAULT_HOTKEY.to_string(),
            clip_output_dir: DEFAULT_CLIP_OUTPUT_DIR.to_string(),
        }
    }
}

/// Configuration entry for a single monitored game application.
#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationConfig {
    /// Human-readable name shown in the GUI and used as the clip subdirectory name.
    pub display_name: String,
    /// Executable filename (e.g. "RocketLeague.exe") used for process detection.
    pub executable_name: String,
    /// Overrides the global buffer length for this application (seconds).
    pub buffer_length_secs: Option<u32>,
    /// Overrides the global hotkey for this application.
    pub hotkey: Option<String>,
}

impl ApplicationConfig {
    /// Returns the effective buffer length, falling back to the global config.
    pub fn effective_buffer_length(&self, global: &GlobalConfig) -> u32 {
        let raw = self.buffer_length_secs.unwrap_or(global.buffer_length_secs);
        raw.clamp(MIN_BUFFER_LENGTH_SECS, MAX_BUFFER_LENGTH_SECS)
    }

    /// Returns the effective hotkey, falling back to the global config.
    pub fn effective_hotkey<'a>(&'a self, global: &'a GlobalConfig) -> &'a str {
        self.hotkey.as_deref().unwrap_or(&global.hotkey)
    }
}

/// Loads the config file at `path`, returning `Config::default()` if the file does not exist.
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load_or_default(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))
}

/// Spawns a file watcher on the parent directory of `path`.  Whenever the config
/// file is created or modified, reloads it and sends a `ConfigReloaded` event.
pub async fn watch_config(path: PathBuf, tx: mpsc::Sender<DaemonEvent>) {
    let (watch_tx, mut watch_rx) = mpsc::channel::<notify::Event>(16);

    let mut watcher = match RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = watch_tx.blocking_send(event);
            }
        },
        NotifyConfig::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[config] Failed to create file watcher: {e}");
            return;
        }
    };

    // Watch the parent directory rather than the file directly so we catch
    // editor-style atomic saves (write-new + rename).
    let watch_dir = match path.parent() {
        Some(d) => d.to_path_buf(),
        None => {
            eprintln!("[config] Config path has no parent directory");
            return;
        }
    };

    if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
        eprintln!("[config] Failed to watch config directory: {e}");
        return;
    }

    while let Some(event) = watch_rx.recv().await {
        let affects_config = event.paths.iter().any(|p| p == path.as_path());
        let is_write = matches!(
            event.kind,
            notify::EventKind::Create(_) | notify::EventKind::Modify(_)
        );

        if affects_config && is_write {
            match load_or_default(&path) {
                Ok(config) => {
                    if tx.send(DaemonEvent::ConfigReloaded(config)).await.is_err() {
                        break;
                    }
                }
                Err(e) => eprintln!("[config] Failed to reload config: {e}"),
            }
        }
    }
}

fn default_buffer_length() -> u32 {
    DEFAULT_BUFFER_LENGTH_SECS
}

fn default_hotkey() -> String {
    DEFAULT_HOTKEY.to_string()
}

fn default_clip_output_dir() -> String {
    DEFAULT_CLIP_OUTPUT_DIR.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_global(buffer_secs: u32) -> GlobalConfig {
        GlobalConfig {
            buffer_length_secs: buffer_secs,
            hotkey: "F8".to_string(),
            clip_output_dir: DEFAULT_CLIP_OUTPUT_DIR.to_string(),
        }
    }

    fn make_app(buffer_override: Option<u32>, hotkey_override: Option<&str>) -> ApplicationConfig {
        ApplicationConfig {
            display_name: "Test Game".to_string(),
            executable_name: "game.exe".to_string(),
            buffer_length_secs: buffer_override,
            hotkey: hotkey_override.map(|s| s.to_string()),
        }
    }

    // ── defaults ──────────────────────────────────────────────────────────────

    #[test]
    fn global_config_default_values() {
        let g = GlobalConfig::default();
        assert_eq!(g.buffer_length_secs, DEFAULT_BUFFER_LENGTH_SECS);
        assert_eq!(g.hotkey, DEFAULT_HOTKEY);
        assert_eq!(g.clip_output_dir, DEFAULT_CLIP_OUTPUT_DIR);
    }

    #[test]
    fn config_default_has_no_applications() {
        let c = Config::default();
        assert!(c.applications.is_empty());
    }

    // ── effective_buffer_length ───────────────────────────────────────────────

    #[test]
    fn effective_buffer_length_uses_app_override() {
        let global = make_global(DEFAULT_BUFFER_LENGTH_SECS);
        let app = make_app(Some(30), None);
        assert_eq!(app.effective_buffer_length(&global), 30);
    }

    #[test]
    fn effective_buffer_length_falls_back_to_global() {
        let global = make_global(20);
        let app = make_app(None, None);
        assert_eq!(app.effective_buffer_length(&global), 20);
    }

    #[test]
    fn effective_buffer_length_clamps_below_min() {
        let global = make_global(DEFAULT_BUFFER_LENGTH_SECS);
        let app = make_app(Some(1), None);
        assert_eq!(app.effective_buffer_length(&global), MIN_BUFFER_LENGTH_SECS);
    }

    #[test]
    fn effective_buffer_length_clamps_above_max() {
        let global = make_global(DEFAULT_BUFFER_LENGTH_SECS);
        let app = make_app(Some(999), None);
        assert_eq!(app.effective_buffer_length(&global), MAX_BUFFER_LENGTH_SECS);
    }

    #[test]
    fn effective_buffer_length_clamps_global_fallback_below_min() {
        // Even the global value is clamped when the app has no override.
        let global = make_global(2);
        let app = make_app(None, None);
        assert_eq!(app.effective_buffer_length(&global), MIN_BUFFER_LENGTH_SECS);
    }

    #[test]
    fn effective_buffer_length_at_exact_min_and_max() {
        let global = make_global(DEFAULT_BUFFER_LENGTH_SECS);
        let at_min = make_app(Some(MIN_BUFFER_LENGTH_SECS), None);
        let at_max = make_app(Some(MAX_BUFFER_LENGTH_SECS), None);
        assert_eq!(at_min.effective_buffer_length(&global), MIN_BUFFER_LENGTH_SECS);
        assert_eq!(at_max.effective_buffer_length(&global), MAX_BUFFER_LENGTH_SECS);
    }

    // ── effective_hotkey ──────────────────────────────────────────────────────

    #[test]
    fn effective_hotkey_uses_app_override() {
        let global = make_global(DEFAULT_BUFFER_LENGTH_SECS);
        let app = make_app(None, Some("F9"));
        assert_eq!(app.effective_hotkey(&global), "F9");
    }

    #[test]
    fn effective_hotkey_falls_back_to_global() {
        let global = make_global(DEFAULT_BUFFER_LENGTH_SECS);
        let app = make_app(None, None);
        assert_eq!(app.effective_hotkey(&global), DEFAULT_HOTKEY);
    }

    // ── load_or_default ───────────────────────────────────────────────────────

    #[test]
    fn load_or_default_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let config = load_or_default(&path).unwrap();
        assert_eq!(config.global.buffer_length_secs, DEFAULT_BUFFER_LENGTH_SECS);
        assert!(config.applications.is_empty());
    }

    #[test]
    fn load_or_default_parses_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[global]
buffer_length_secs = 45
hotkey = "F10"
clip_output_dir = "C:\\Clips"

[[applications]]
display_name = "Rocket League"
executable_name = "RocketLeague.exe"
"#,
        )
        .unwrap();

        let config = load_or_default(&path).unwrap();
        assert_eq!(config.global.buffer_length_secs, 45);
        assert_eq!(config.global.hotkey, "F10");
        assert_eq!(config.global.clip_output_dir, "C:\\Clips");
        assert_eq!(config.applications.len(), 1);
        assert_eq!(config.applications[0].display_name, "Rocket League");
        assert_eq!(config.applications[0].executable_name, "RocketLeague.exe");
        assert!(config.applications[0].buffer_length_secs.is_none());
        assert!(config.applications[0].hotkey.is_none());
    }

    #[test]
    fn load_or_default_partial_toml_uses_field_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // Only override one field; the rest should get their defaults.
        std::fs::write(&path, "[global]\nbuffer_length_secs = 60\n").unwrap();

        let config = load_or_default(&path).unwrap();
        assert_eq!(config.global.buffer_length_secs, 60);
        assert_eq!(config.global.hotkey, DEFAULT_HOTKEY);
        assert_eq!(config.global.clip_output_dir, DEFAULT_CLIP_OUTPUT_DIR);
    }

    #[test]
    fn load_or_default_invalid_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is not valid toml ][[[").unwrap();
        assert!(load_or_default(&path).is_err());
    }

    #[test]
    fn load_or_default_app_with_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[[applications]]
display_name = "Fortnite"
executable_name = "FortniteClient-Win64-Shipping.exe"
buffer_length_secs = 30
hotkey = "F7"
"#,
        )
        .unwrap();

        let config = load_or_default(&path).unwrap();
        let app = &config.applications[0];
        assert_eq!(app.buffer_length_secs, Some(30));
        assert_eq!(app.hotkey.as_deref(), Some("F7"));
    }
}
