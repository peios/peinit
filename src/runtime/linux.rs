mod calendar_timer;
mod model;
mod setup;
mod timer_last_run;
mod turn;

pub use model::{DEFAULT_MAX_RUNTIME_EVENTS, LinuxRuntimeConfig, LinuxRuntimeSetupError};

use std::path::Path;

use crate::boundary::{
    LinuxBootAttemptCounter, LinuxChildReaper, LinuxConsoleSink, LinuxEpoll,
    LinuxFilesystemCheckHelper, LinuxJobIdentityProvider, LinuxKmesEventSink, LinuxMonotonicClock,
    LinuxPid1SignalFd, LinuxPowerButtonDevices, LinuxProcessController, LinuxProcessLauncher,
    LinuxSystemTokenProvider, LinuxTimerFd,
};
use crate::control::connection::{ControlConnectionRecord, ControlConnectionTable};
use crate::control::socket::{LinuxControlConnection, LinuxControlSocket};
use crate::control::system::PeiosSystemAccessChecker;
use crate::jobs::socket::LinuxJobsSocket;
use crate::notify::NotifySocket;
#[cfg(feature = "peios-registry")]
use crate::registry::{LcsRegistryClient, LcsRegistryWatches};
use crate::runtime::{RuntimeJobsChannelTable, RuntimeServiceLogPipes};
use crate::shutdown::LinuxShutdownFinalizer;

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
    /// `peios.quiet`, from the kernel command line.
    quiet: crate::init::QuietLevel,
    /// Re-evaluated once per turn rather than per message: terminal ownership
    /// can only change when a service does, and a turn is the granularity at
    /// which that happens.
    quiet_policy: crate::runtime::console::QuietPolicy,
    log_pipes: RuntimeServiceLogPipes,
    jobs_channel: RuntimeJobsChannelTable<LinuxJobsSocket>,
    /// Refreshed from the supervisor every turn, like the control limits.
    jobs_connection_timeout_secs: u64,
    job_identity_provider: LinuxJobIdentityProvider,
    power_buttons: LinuxPowerButtonDevices,
    clock: LinuxMonotonicClock,
    controller: LinuxProcessController,
    boot_attempt_counter: LinuxBootAttemptCounter,
    token_provider: LinuxSystemTokenProvider,
    process_launcher: LinuxProcessLauncher,
    filesystem_check_launcher: LinuxFilesystemCheckHelper,
    /// Outstanding forked timer last-run writes, so a failed one is reported
    /// rather than discarded with the child (PEI-369).
    timer_last_run_writes: timer_last_run::TimerLastRunWrites,
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

    pub fn jobs_socket_path(&self) -> &Path {
        self.jobs_channel.listener().path()
    }

    pub fn active_jobs_connections(&self) -> usize {
        self.jobs_channel.active_connections()
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

    /// `peios.notifysocket=` reaches the runtime: the socket the runtime
    /// binds is the one Phase 1 bound and services were told about, not the
    /// default beside it (PEI-804).
    #[test]
    fn linux_runtime_config_for_settings_carries_the_notify_socket_and_quiet_level() {
        let mut settings = SupervisorSettings::default();
        settings.notify_socket_path = "/run/alt/notify.sock".to_string();
        settings.quiet = crate::init::QuietLevel::Blackout;

        let config = LinuxRuntimeConfig::for_settings(&settings);

        assert_eq!(
            config.notify_socket_path,
            std::path::PathBuf::from("/run/alt/notify.sock")
        );
        assert_eq!(config.quiet, crate::init::QuietLevel::Blackout);
        assert_eq!(
            config.control_socket_path,
            std::path::PathBuf::from(CONTROL_SOCKET_PATH)
        );
        assert_eq!(config.max_events, DEFAULT_MAX_RUNTIME_EVENTS);
    }

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
