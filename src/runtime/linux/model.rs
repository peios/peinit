use std::path::PathBuf;

use crate::boundary::{
    BoundaryError, LinuxEpollCreateError, LinuxTimerFdCreateError, Pid1SignalFdRegisteredSetupError,
};
use crate::control::socket::{
    CONTROL_SOCKET_PATH, ControlSocketBindError, DEFAULT_MAX_CONTROL_CONNECTIONS,
};
use crate::control::system::ControlSecurityDescriptor;
use crate::jobs::socket::{DEFAULT_MAX_JOBS_CONNECTIONS, JOBS_SOCKET_PATH};
use crate::notify::NotifySocketBindError;
use crate::runtime::{RuntimeControlLimits, RuntimeEventRegistrationError, RuntimeWorkPumpConfig};
use crate::supervisor::SupervisorSettings;

pub const DEFAULT_MAX_RUNTIME_EVENTS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxRuntimeConfig {
    pub control_socket_path: PathBuf,
    pub jobs_socket_path: PathBuf,
    pub notify_socket_path: PathBuf,
    pub max_events: usize,
    pub max_control_connections: usize,
    pub max_jobs_connections: usize,
    pub control_limits: RuntimeControlLimits,
    pub control_security: ControlSecurityDescriptor,
    pub work_pump: RuntimeWorkPumpConfig,
    /// `peios.quiet` — how much peinit may write to the console.
    pub quiet: crate::init::QuietLevel,
}

impl LinuxRuntimeConfig {
    /// The runtime configuration for the supervisor Phase 1 hands over.
    ///
    /// The per-boot command-line values the supervisor carries apply to the
    /// runtime too: `peios.quiet`, and the notify socket path, which Phase 1
    /// bound and every service's NOTIFY_SOCKET names. The runtime rebinds
    /// that path, so it has to be the same one, or nothing listens where
    /// services were told to write (PEI-804).
    pub fn for_settings(settings: &SupervisorSettings) -> Self {
        Self {
            notify_socket_path: PathBuf::from(&settings.notify_socket_path),
            quiet: settings.quiet,
            ..Self::default()
        }
    }
}

impl Default for LinuxRuntimeConfig {
    fn default() -> Self {
        Self {
            control_socket_path: PathBuf::from(CONTROL_SOCKET_PATH),
            jobs_socket_path: PathBuf::from(JOBS_SOCKET_PATH),
            notify_socket_path: PathBuf::from(SupervisorSettings::DEFAULT_NOTIFY_SOCKET_PATH),
            max_events: DEFAULT_MAX_RUNTIME_EVENTS,
            max_control_connections: DEFAULT_MAX_CONTROL_CONNECTIONS,
            max_jobs_connections: DEFAULT_MAX_JOBS_CONNECTIONS,
            control_limits: RuntimeControlLimits::default(),
            control_security: ControlSecurityDescriptor::Default,
            work_pump: RuntimeWorkPumpConfig::default(),
            quiet: crate::init::QuietLevel::default(),
        }
    }
}

#[derive(Debug)]
pub enum LinuxRuntimeSetupError {
    MissingControlSocket,
    MissingJobsSocket,
    JobsSocket(crate::jobs::socket::JobsSocketBindError),
    Epoll(LinuxEpollCreateError),
    Signal(Pid1SignalFdRegisteredSetupError),
    ControlSocket(ControlSocketBindError),
    NotifySocket(NotifySocketBindError),
    Register(RuntimeEventRegistrationError),
    Timer(LinuxTimerFdCreateError),
    RegistryWatch(BoundaryError),
}
