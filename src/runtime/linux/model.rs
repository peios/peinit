use std::path::PathBuf;

use crate::boundary::{
    BoundaryError, LinuxEpollCreateError, LinuxTimerFdCreateError, Pid1SignalFdRegisteredSetupError,
};
use crate::control::socket::{
    CONTROL_SOCKET_PATH, ControlSocketBindError, DEFAULT_MAX_CONTROL_CONNECTIONS,
};
use crate::control::system::ControlSecurityDescriptor;
use crate::notify::NotifySocketBindError;
use crate::runtime::{RuntimeControlLimits, RuntimeEventRegistrationError, RuntimeWorkPumpConfig};
use crate::supervisor::SupervisorSettings;

pub const DEFAULT_MAX_RUNTIME_EVENTS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxRuntimeConfig {
    pub control_socket_path: PathBuf,
    pub notify_socket_path: PathBuf,
    pub max_events: usize,
    pub max_control_connections: usize,
    pub control_limits: RuntimeControlLimits,
    pub control_security: ControlSecurityDescriptor,
    pub work_pump: RuntimeWorkPumpConfig,
}

impl Default for LinuxRuntimeConfig {
    fn default() -> Self {
        Self {
            control_socket_path: PathBuf::from(CONTROL_SOCKET_PATH),
            notify_socket_path: PathBuf::from(SupervisorSettings::DEFAULT_NOTIFY_SOCKET_PATH),
            max_events: DEFAULT_MAX_RUNTIME_EVENTS,
            max_control_connections: DEFAULT_MAX_CONTROL_CONNECTIONS,
            control_limits: RuntimeControlLimits::default(),
            control_security: ControlSecurityDescriptor::Default,
            work_pump: RuntimeWorkPumpConfig::default(),
        }
    }
}

#[derive(Debug)]
pub enum LinuxRuntimeSetupError {
    MissingControlSocket,
    Epoll(LinuxEpollCreateError),
    Signal(Pid1SignalFdRegisteredSetupError),
    ControlSocket(ControlSocketBindError),
    NotifySocket(NotifySocketBindError),
    Register(RuntimeEventRegistrationError),
    Timer(LinuxTimerFdCreateError),
    RegistryWatch(BoundaryError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase1InfrastructureRegistration {
    pub jfs: Phase1JfsRegistration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase1JfsRegistration {
    Unavailable,
    Registered {
        fd: i32,
        path: String,
    },
    RegisterFailed {
        fd: i32,
        path: String,
        error: RuntimeEventRegistrationError,
    },
}
