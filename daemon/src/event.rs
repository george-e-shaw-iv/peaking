use crate::config::{ApplicationConfig, Config};

pub enum DaemonEvent {
    /// A watched game process appeared in the process list.
    ProcessStarted(ApplicationConfig),
    /// The previously active watched process exited.
    ProcessStopped,
    /// The config file changed on disk and was successfully re-parsed.
    ConfigReloaded(Config),
    /// The clip hotkey was pressed; flush the ring buffer to disk.
    /// Implemented in Phase 8 (hotkey) and consumed in Phase 9 (flush).
    FlushRequested,
    /// Ctrl+C received; the daemon should flush state and exit.
    Shutdown,
}
