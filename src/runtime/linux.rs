mod calendar_timer;
mod model;
mod setup;
mod turn;

pub use model::{
    DEFAULT_MAX_RUNTIME_EVENTS, LinuxRuntimeConfig, LinuxRuntimeSetupError,
    Phase1InfrastructureRegistration, Phase1JfsRegistration,
};

use std::path::Path;

use crate::boundary::{
    LinuxBootAttemptCounter, LinuxChildReaper, LinuxConsoleSink, LinuxEpoll,
    LinuxFilesystemCheckHelper, LinuxKmesEventSink, LinuxMonotonicClock, LinuxPid1SignalFd,
    LinuxProcessController, LinuxProcessLauncher, LinuxSystemTokenProvider, LinuxTimerFd,
};
use crate::control::connection::{ControlConnectionRecord, ControlConnectionTable};
use crate::control::socket::{LinuxControlConnection, LinuxControlSocket};
use crate::control::system::PeiosSystemAccessChecker;
use crate::notify::NotifySocket;
#[cfg(feature = "peios-registry")]
use crate::registry::{LcsRegistryClient, LcsRegistryWatches};
use crate::runtime::RuntimeServiceLogPipes;
use crate::shutdown::LinuxShutdownFinalizer;

use super::jfs::RuntimeJfsDevice;

#[derive(Debug)]
pub struct LinuxShutdownRuntime {
    epoll: LinuxEpoll,
    signal: LinuxPid1SignalFd,
    child_reaper: LinuxChildReaper,
    notify_socket: NotifySocket,
    control_listener: LinuxControlSocket,
    control_connections: ControlConnectionTable<ControlConnectionRecord<LinuxControlConnection>>,
    deadline_timer: LinuxTimerFd,
    lifecycle_timer: LinuxTimerFd,
    calendar_timers: calendar_timer::LinuxCalendarTimerTable,
    kmes_sink: LinuxKmesEventSink,
    console_sink: LinuxConsoleSink,
    log_pipes: RuntimeServiceLogPipes,
    jfs_device: Option<RuntimeJfsDevice>,
    clock: LinuxMonotonicClock,
    controller: LinuxProcessController,
    boot_attempt_counter: LinuxBootAttemptCounter,
    token_provider: LinuxSystemTokenProvider,
    process_launcher: LinuxProcessLauncher,
    filesystem_check_launcher: LinuxFilesystemCheckHelper,
    filesystem_check_reader: LinuxFilesystemCheckHelper,
    finalizer: LinuxShutdownFinalizer,
    access_checker: PeiosSystemAccessChecker,
    #[cfg(feature = "peios-registry")]
    registry: LcsRegistryClient,
    #[cfg(feature = "peios-registry")]
    registry_watches: LcsRegistryWatches,
    config: LinuxRuntimeConfig,
}

impl LinuxShutdownRuntime {
    pub fn control_socket_path(&self) -> &Path {
        self.control_listener.path()
    }

    pub fn active_control_connections(&self) -> usize {
        self.control_connections.len()
    }

    pub fn notify_socket_path(&self) -> &Path {
        self.notify_socket.path()
    }

    pub fn jfs_device_fd(&self) -> Option<i32> {
        self.jfs_device.as_ref().map(RuntimeJfsDevice::fd)
    }
}

#[cfg(test)]
mod tests {
    use crate::control::socket::{
        CONTROL_SOCKET_PATH, DEFAULT_CONNECTION_TIMEOUT_SECS, DEFAULT_MAX_CONTROL_CONNECTIONS,
        DEFAULT_MAX_REQUEST_SIZE_BYTES,
    };
    use crate::control::system::ControlSecurityDescriptor;
    use crate::runtime::{DEFAULT_MAX_CONTROL_READ_BYTES, RuntimeWorkPumpConfig};
    use crate::supervisor::SupervisorSettings;

    use super::{DEFAULT_MAX_RUNTIME_EVENTS, LinuxRuntimeConfig};

    #[test]
    fn linux_runtime_config_defaults_match_control_limits() {
        let config = LinuxRuntimeConfig::default();

        assert_eq!(
            config.control_socket_path,
            std::path::PathBuf::from(CONTROL_SOCKET_PATH)
        );
        assert_eq!(
            config.notify_socket_path,
            std::path::PathBuf::from(SupervisorSettings::DEFAULT_NOTIFY_SOCKET_PATH)
        );
        assert_eq!(config.max_events, DEFAULT_MAX_RUNTIME_EVENTS);
        assert_eq!(
            config.max_control_connections,
            DEFAULT_MAX_CONTROL_CONNECTIONS
        );
        assert_eq!(
            config.control_limits.max_read_bytes,
            DEFAULT_MAX_CONTROL_READ_BYTES
        );
        assert_eq!(
            config.control_limits.max_request_bytes,
            DEFAULT_MAX_REQUEST_SIZE_BYTES,
        );
        assert_eq!(
            config.control_limits.connection_timeout_secs,
            DEFAULT_CONNECTION_TIMEOUT_SECS,
        );
        assert_eq!(config.control_security, ControlSecurityDescriptor::Default);
        assert_eq!(config.work_pump, RuntimeWorkPumpConfig::default());
    }
}
