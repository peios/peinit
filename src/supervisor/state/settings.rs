use crate::boot::phase2::{DEFAULT_MAX_PARALLEL_STARTS, Phase2BootSettings};
use crate::shutdown::ShutdownSettings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSettings {
    pub phase2: Phase2BootSettings,
    pub notify_socket_path: String,
    pub shutdown: ShutdownSettings,
}

impl SupervisorSettings {
    pub const DEFAULT_NOTIFY_SOCKET_PATH: &'static str = "/run/peinit/notify.sock";

    pub fn new(phase2: Phase2BootSettings) -> Self {
        Self {
            phase2,
            notify_socket_path: Self::DEFAULT_NOTIFY_SOCKET_PATH.to_string(),
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
            shutdown: ShutdownSettings::default(),
        }
    }
}
