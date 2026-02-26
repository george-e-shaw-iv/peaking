/// Global hotkey listener using a low-level Windows keyboard hook (`WH_KEYBOARD_LL`).
///
/// The hook runs on a dedicated OS thread with its own Windows message pump, so it
/// fires even when a full-screen exclusive-mode game has focus.  The hook thread exits
/// cleanly when [`HotkeyHandle::stop`] is called.
///
/// On non-Windows platforms the public API compiles but is a no-op at runtime.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use tokio::sync::mpsc;

use crate::event::DaemonEvent;

/// Currently watched virtual-key code (0 = disabled).
/// Written by [`HotkeyHandle::update_key`]; read inside the hook callback.
static HOOK_VK: AtomicU32 = AtomicU32::new(0);

/// Tokio channel used to forward [`DaemonEvent::FlushRequested`] from the hook
/// callback to the main event loop.  Set once by [`start`].
static HOOK_TX: OnceLock<mpsc::Sender<DaemonEvent>> = OnceLock::new();

/// Converts a hotkey name string (e.g. `"F8"`, `"A"`) to a Windows virtual-key code.
///
/// Supported keys:
/// - Function keys `F1`–`F12` (case-insensitive).
/// - ASCII letters `A`–`Z` (normalised to their uppercase VK values, `0x41`–`0x5A`).
/// - ASCII digits `0`–`9` (VK values `0x30`–`0x39`).
///
/// Returns `None` for any unrecognised name.
pub fn parse_vk(name: &str) -> Option<u32> {
    match name.to_uppercase().as_str() {
        "F1"  => Some(0x70),
        "F2"  => Some(0x71),
        "F3"  => Some(0x72),
        "F4"  => Some(0x73),
        "F5"  => Some(0x74),
        "F6"  => Some(0x75),
        "F7"  => Some(0x76),
        "F8"  => Some(0x77),
        "F9"  => Some(0x78),
        "F10" => Some(0x79),
        "F11" => Some(0x7A),
        "F12" => Some(0x7B),
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap();
            if c.is_ascii_alphanumeric() {
                // 'A'=0x41…'Z'=0x5A; '0'=0x30…'9'=0x39 — exact match to Windows VK codes.
                Some(c.to_ascii_uppercase() as u32)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ── Public handle ─────────────────────────────────────────────────────────────

/// A handle to the running keyboard hook.
///
/// Allows updating the key binding on config reload and stopping the hook
/// thread when the daemon exits.
pub struct HotkeyHandle {
    #[cfg(windows)]
    _thread: std::thread::JoinHandle<()>,
    /// Thread ID of the message-pump thread, used to post `WM_QUIT`.
    #[cfg(windows)]
    thread_id: u32,
}

impl HotkeyHandle {
    /// Changes the active hotkey to `hotkey_name`.
    ///
    /// Pass an unrecognised name (e.g. `""`) to disable the hotkey without
    /// stopping the hook thread.
    pub fn update_key(&self, hotkey_name: &str) {
        HOOK_VK.store(parse_vk(hotkey_name).unwrap_or(0), Ordering::Relaxed);
    }

    /// Signals the hook thread to stop and blocks until it exits.
    pub fn stop(self) {
        #[cfg(windows)]
        {
            imp::post_quit(self.thread_id);
            let _ = self._thread.join();
        }
    }
}

// ── Startup ───────────────────────────────────────────────────────────────────

/// Installs a `WH_KEYBOARD_LL` keyboard hook on a dedicated OS thread and
/// returns a [`HotkeyHandle`] for managing it.
///
/// When the configured key is pressed, [`DaemonEvent::FlushRequested`] is sent
/// to `tx` via a non-blocking [`try_send`](mpsc::Sender::try_send).  If the
/// channel is full the hotkey press is silently dropped for that cycle.
///
/// # Windows
/// Panics if `SetWindowsHookExW` fails.
///
/// # Non-Windows
/// Returns a stub handle; all methods compile and run but do nothing.
pub fn start(initial_hotkey: &str, tx: mpsc::Sender<DaemonEvent>) -> HotkeyHandle {
    HOOK_VK.store(parse_vk(initial_hotkey).unwrap_or(0), Ordering::Relaxed);
    // Silently ignore if called more than once (e.g. in test binaries).
    let _ = HOOK_TX.set(tx);

    #[cfg(windows)]
    {
        let (id_tx, id_rx) = std::sync::mpsc::sync_channel::<u32>(1);
        let thread = std::thread::Builder::new()
            .name("hotkey-pump".into())
            .spawn(move || imp::run_message_pump(id_tx))
            .expect("Failed to spawn hotkey thread");
        let thread_id = id_rx.recv().expect("hotkey thread did not send its ID");
        HotkeyHandle { _thread: thread, thread_id }
    }

    #[cfg(not(windows))]
    HotkeyHandle {}
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc as std_mpsc;

    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW,
        SetWindowsHookExW, UnhookWindowsHookEx,
        KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT,
    };

    use crate::event::DaemonEvent;
    use super::{HOOK_TX, HOOK_VK};

    /// Low-level keyboard hook procedure.
    ///
    /// Called by Windows on every keyboard event system-wide.  We only act when
    /// `nCode >= 0` and the virtual-key code matches the configured target.
    unsafe extern "system" fn keyboard_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 && w_param.0 as u32 == WM_KEYDOWN {
            let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            let target = HOOK_VK.load(Ordering::Relaxed);
            if target != 0 && kb.vkCode == target {
                if let Some(tx) = HOOK_TX.get() {
                    // try_send is non-blocking; a full channel silently drops this press.
                    let _ = tx.try_send(DaemonEvent::FlushRequested);
                }
            }
        }
        CallNextHookEx(None, n_code, w_param, l_param)
    }

    /// Installs `WH_KEYBOARD_LL`, runs a Windows message pump until `WM_QUIT`,
    /// then uninstalls the hook.
    ///
    /// Sends the current thread ID to `id_tx` before entering the pump so
    /// that [`super::start`] can later use it to post `WM_QUIT`.
    pub fn run_message_pump(id_tx: std_mpsc::SyncSender<u32>) {
        unsafe {
            let _ = id_tx.send(GetCurrentThreadId());
            drop(id_tx);

            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_proc),
                HINSTANCE::default(),
                0,
            )
            .expect("SetWindowsHookExW failed");

            let mut msg = MSG::default();
            // GetMessageW: >0 = message, 0 = WM_QUIT, <0 = error.
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                DispatchMessageW(&msg);
            }

            let _ = UnhookWindowsHookEx(hook);
            eprintln!("[hotkey] Hook thread exited");
        }
    }

    /// Posts `WM_QUIT` to `thread_id`, causing its `GetMessageW` loop to exit.
    pub fn post_quit(thread_id: u32) {
        unsafe {
            let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_vk: function keys ───────────────────────────────────────────────

    #[test]
    fn parse_vk_f1_through_f12() {
        assert_eq!(parse_vk("F1"),  Some(0x70));
        assert_eq!(parse_vk("F2"),  Some(0x71));
        assert_eq!(parse_vk("F3"),  Some(0x72));
        assert_eq!(parse_vk("F4"),  Some(0x73));
        assert_eq!(parse_vk("F5"),  Some(0x74));
        assert_eq!(parse_vk("F6"),  Some(0x75));
        assert_eq!(parse_vk("F7"),  Some(0x76));
        assert_eq!(parse_vk("F8"),  Some(0x77));
        assert_eq!(parse_vk("F9"),  Some(0x78));
        assert_eq!(parse_vk("F10"), Some(0x79));
        assert_eq!(parse_vk("F11"), Some(0x7A));
        assert_eq!(parse_vk("F12"), Some(0x7B));
    }

    #[test]
    fn parse_vk_f_keys_case_insensitive() {
        assert_eq!(parse_vk("f1"),  parse_vk("F1"));
        assert_eq!(parse_vk("f8"),  parse_vk("F8"));
        assert_eq!(parse_vk("f12"), parse_vk("F12"));
    }

    #[test]
    fn f_keys_are_contiguous_from_0x70() {
        for n in 1u32..=12 {
            let name = format!("F{n}");
            let expected = 0x6F + n; // F1=0x70 … F12=0x7B
            assert_eq!(parse_vk(&name), Some(expected), "Wrong VK for {name}");
        }
    }

    // ── parse_vk: letters ─────────────────────────────────────────────────────

    #[test]
    fn parse_vk_letters_match_ascii_uppercase() {
        for c in b'A'..=b'Z' {
            let name = (c as char).to_string();
            assert_eq!(parse_vk(&name), Some(c as u32), "Failed for {name}");
        }
    }

    #[test]
    fn parse_vk_lowercase_letters_normalised_to_uppercase() {
        for c in b'a'..=b'z' {
            let lower = (c as char).to_string();
            let upper = lower.to_uppercase();
            assert_eq!(parse_vk(&lower), parse_vk(&upper));
        }
    }

    // ── parse_vk: digits ──────────────────────────────────────────────────────

    #[test]
    fn parse_vk_digits_match_ascii() {
        for c in b'0'..=b'9' {
            let name = (c as char).to_string();
            assert_eq!(parse_vk(&name), Some(c as u32), "Failed for {name}");
        }
    }

    // ── parse_vk: unrecognised ────────────────────────────────────────────────

    #[test]
    fn parse_vk_empty_string() {
        assert_eq!(parse_vk(""), None);
    }

    #[test]
    fn parse_vk_f0_is_not_a_key() {
        assert_eq!(parse_vk("F0"), None);
    }

    #[test]
    fn parse_vk_f13_and_above_return_none() {
        assert_eq!(parse_vk("F13"), None);
        assert_eq!(parse_vk("F24"), None);
    }

    #[test]
    fn parse_vk_multi_char_non_f_names_return_none() {
        assert_eq!(parse_vk("Escape"), None);
        assert_eq!(parse_vk("Enter"), None);
        assert_eq!(parse_vk("Space"), None);
        assert_eq!(parse_vk("AB"), None);
    }

    #[test]
    fn parse_vk_special_chars_return_none() {
        assert_eq!(parse_vk("!"), None);
        assert_eq!(parse_vk("@"), None);
        assert_eq!(parse_vk(" "), None);
        assert_eq!(parse_vk("\t"), None);
    }

    // ── Windows: HotkeyHandle lifecycle ───────────────────────────────────────

    /// Exercises the full `start → update_key → stop` lifecycle on Windows and
    /// verifies that `update_key` writes the expected virtual-key code into the
    /// `HOOK_VK` atomic that the hook callback reads.
    ///
    /// Only one test calls `start()` to avoid installing multiple
    /// `WH_KEYBOARD_LL` hooks in the same test binary.
    #[cfg(windows)]
    #[test]
    fn lifecycle_start_update_key_stop_does_not_panic() {
        use crate::event::DaemonEvent;
        use std::sync::atomic::Ordering;

        let (tx, _rx) = tokio::sync::mpsc::channel::<DaemonEvent>(8);
        let handle = start("F8", tx);

        // The initial key must be stored immediately.
        assert_eq!(
            HOOK_VK.load(Ordering::Relaxed),
            parse_vk("F8").unwrap(),
            "HOOK_VK should contain the F8 VK code after start()"
        );

        // update_key stores the VK code for the new key.
        handle.update_key("F9");
        assert_eq!(HOOK_VK.load(Ordering::Relaxed), parse_vk("F9").unwrap());

        handle.update_key("Z");
        assert_eq!(HOOK_VK.load(Ordering::Relaxed), parse_vk("Z").unwrap());

        // An unrecognised key name disables the hotkey (stores 0).
        handle.update_key("NotAKey");
        assert_eq!(HOOK_VK.load(Ordering::Relaxed), 0);

        handle.stop();
    }

}
