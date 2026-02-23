/// Canonical file paths for Peaking data files on Windows.
///
/// Both files live under %APPDATA%\Peaking\:
///   - config.toml  Written by the GUI, read by the daemon.
///   - status.toml  Written by the daemon, read by the GUI.
use std::path::PathBuf;

const APP_DIR_NAME: &str = "Peaking";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const STATUS_FILE_NAME: &str = "status.toml";

/// Returns the Peaking application data directory: %APPDATA%\Peaking\
pub fn app_data_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA environment variable not set");
    PathBuf::from(appdata).join(APP_DIR_NAME)
}

/// Returns the full path to the config file: %APPDATA%\Peaking\config.toml
pub fn config_file_path() -> PathBuf {
    app_data_dir().join(CONFIG_FILE_NAME)
}

/// Returns the full path to the status file: %APPDATA%\Peaking\status.toml
pub fn status_file_path() -> PathBuf {
    app_data_dir().join(STATUS_FILE_NAME)
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn app_data_dir_ends_with_peaking() {
        let dir = app_data_dir();
        assert_eq!(dir.file_name().unwrap(), "Peaking");
    }

    #[test]
    fn app_data_dir_is_inside_appdata() {
        let appdata = std::env::var("APPDATA").unwrap();
        let dir = app_data_dir();
        assert!(dir.starts_with(&appdata));
    }

    #[test]
    fn config_file_path_has_correct_name() {
        let path = config_file_path();
        assert_eq!(path.file_name().unwrap(), CONFIG_FILE_NAME);
    }

    #[test]
    fn status_file_path_has_correct_name() {
        let path = status_file_path();
        assert_eq!(path.file_name().unwrap(), STATUS_FILE_NAME);
    }

    #[test]
    fn config_and_status_share_same_parent_dir() {
        let config = config_file_path();
        let status = status_file_path();
        assert_eq!(config.parent(), status.parent());
    }
}
