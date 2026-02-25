/// Windows startup registration via the `HKCU\...\Run` registry key.
///
/// On first run (and idempotently on every subsequent run) the daemon registers
/// itself so that Windows launches it automatically when the user logs in.
///
/// The registration can be removed by running the daemon with the
/// `--unregister-startup` flag.
///
/// On non-Windows platforms both functions compile and succeed as no-ops.
use anyhow::Result;

// ── Windows implementation ─────────────────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use anyhow::{bail, Result};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows::core::PCWSTR;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "Peaking";

    /// Converts a Rust `&str` to a null-terminated UTF-16 `Vec<u16>`.
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Registers `exe_path` under `HKCU\...\Run\Peaking`.
    /// Idempotent: safe to call even if the value already exists.
    pub fn register(exe_path: &str) -> Result<()> {
        let key_w = to_wide(RUN_KEY);
        let val_w = to_wide(VALUE_NAME);
        let data_w = to_wide(exe_path);
        let data_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(data_w.as_ptr() as *const u8, data_w.len() * 2)
        };

        let mut hkey = HKEY::default();
        let err = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR::from_raw(key_w.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut hkey,
                None,
            )
        };
        if err != ERROR_SUCCESS {
            bail!("RegCreateKeyExW failed: {:?}", err);
        }

        let err = unsafe {
            RegSetValueExW(
                hkey,
                PCWSTR::from_raw(val_w.as_ptr()),
                0,
                REG_SZ,
                Some(data_bytes),
            )
        };
        unsafe { let _ = RegCloseKey(hkey); };

        if err != ERROR_SUCCESS {
            bail!("RegSetValueExW failed: {:?}", err);
        }
        Ok(())
    }

    /// Removes the `Peaking` value from `HKCU\...\Run`.
    /// Succeeds silently if the value or key does not exist.
    pub fn unregister() -> Result<()> {
        let key_w = to_wide(RUN_KEY);
        let val_w = to_wide(VALUE_NAME);

        let mut hkey = HKEY::default();
        let err = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR::from_raw(key_w.as_ptr()),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            )
        };

        if err != ERROR_SUCCESS {
            // Key doesn't exist — nothing to remove.
            return Ok(());
        }

        let err = unsafe {
            RegDeleteValueW(hkey, PCWSTR::from_raw(val_w.as_ptr()))
        };
        unsafe { let _ = RegCloseKey(hkey); };

        // ERROR_FILE_NOT_FOUND means the value was already absent — that's fine.
        if err != ERROR_SUCCESS && err.0 != 2 {
            bail!("RegDeleteValueW failed: {:?}", err);
        }
        Ok(())
    }
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Registers the running daemon binary to launch automatically at user login.
///
/// Uses `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` on Windows.
/// Idempotent — safe to call on every startup.
pub fn register_startup() -> Result<()> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe()
            .map_err(|e| anyhow::anyhow!("Failed to locate daemon executable: {e}"))?;
        let exe_str = exe.to_string_lossy();
        imp::register(&exe_str)?;
        println!("[startup] Registered in Windows startup: {exe_str}");
    }
    #[cfg(not(windows))]
    {
        // No-op on non-Windows platforms.
    }
    Ok(())
}

/// Removes the daemon from the Windows startup registry.
///
/// Intended for use with the `--unregister-startup` CLI flag or an uninstaller.
pub fn unregister_startup() -> Result<()> {
    #[cfg(windows)]
    {
        imp::unregister()?;
        println!("[startup] Removed from Windows startup registry");
    }
    #[cfg(not(windows))]
    {
        // No-op on non-Windows platforms.
    }
    Ok(())
}
