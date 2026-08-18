use crate::boot::phase2::{DEFAULT_MAX_PARALLEL_STARTS, Phase2BootSettings};
use crate::shutdown::ShutdownSettings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSettings {
    pub phase2: Phase2BootSettings,
    pub notify_socket_path: String,
    /// `peios.quiet` — how much peinit may write to the console. A per-boot
    /// command-line value like `phase2.mode`, carried here because the runtime
    /// is entered with a supervisor and nothing else.
    pub quiet: crate::init::QuietLevel,
    pub shutdown: ShutdownSettings,
}

impl SupervisorSettings {
    pub const DEFAULT_NOTIFY_SOCKET_PATH: &'static str = "/run/services/peinit/notify.sock";

    pub fn new(phase2: Phase2BootSettings) -> Self {
        Self {
            phase2,
            notify_socket_path: Self::DEFAULT_NOTIFY_SOCKET_PATH.to_string(),
            quiet: crate::init::QuietLevel::default(),
            shutdown: ShutdownSettings::default(),
        }
    }
}

impl Default for SupervisorSettings {
    fn default() -> Self {
        Self {
            phase2: Phase2BootSettings {
                max_parallel_starts: DEFAULT_MAX_PARALLEL_STARTS,
                ..Phase2BootSettings::default()
            },
            notify_socket_path: Self::DEFAULT_NOTIFY_SOCKET_PATH.to_string(),
            quiet: crate::init::QuietLevel::default(),
            shutdown: ShutdownSettings::default(),
        }
    }
}
