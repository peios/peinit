use crate::boundary::{
    LinuxBootAttemptCounter, LinuxChildReaper, LinuxConsoleSink, LinuxEpoll,
    LinuxFilesystemCheckHelper, LinuxJobIdentityProvider, LinuxMonotonicClock, LinuxPid1SignalFd,
    LinuxProcessController, LinuxProcessLauncher, LinuxSystemTokenProvider, LinuxTimerFd,
};
use crate::control::connection::{ControlConnectionRecord, ControlConnectionTable};
use crate::control::socket::{LinuxControlConnection, LinuxControlSocket};
use crate::control::system::PeiosSystemAccessChecker;
use crate::jobs::socket::LinuxJobsSocket;
use crate::notify::NotifySocket;
#[cfg(feature = "peios-registry")]
use crate::registry::LcsRegistryClient;
#[cfg(feature = "peios-registry")]
use crate::registry::{LcsRegistryWatch, LcsRegistryWatches};
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventSource, RuntimeJobsChannelTable, RuntimeServiceLogPipes,
};
use crate::shutdown::LinuxShutdownFinalizer;

use super::{LinuxRuntimeConfig, LinuxRuntimeSetupError, LinuxShutdownRuntime};

impl LinuxShutdownRuntime {
    pub fn setup(config: LinuxRuntimeConfig) -> Result<Self, LinuxRuntimeSetupError> {
        let control_listener = LinuxControlSocket::bind(&config.control_socket_path)
            .map_err(LinuxRuntimeSetupError::ControlSocket)?;
        let jobs_listener = LinuxJobsSocket::bind(&config.jobs_socket_path)
            .map_err(LinuxRuntimeSetupError::JobsSocket)?;
        Self::setup_with_listeners(config, control_listener, jobs_listener)
    }

    pub fn setup_with_infrastructure(
        config: LinuxRuntimeConfig,
        infrastructure: &mut crate::init::Phase1Infrastructure,
    ) -> Result<Self, LinuxRuntimeSetupError> {
        let control_listener = infrastructure
            .take_control_socket()
            .ok_or(LinuxRuntimeSetupError::MissingControlSocket)?;
        let jobs_listener = infrastructure
            .take_jobs_socket()
            .ok_or(LinuxRuntimeSetupError::MissingJobsSocket)?;
        Self::setup_with_listeners(config, control_listener, jobs_listener)
    }

    fn setup_with_listeners(
        config: LinuxRuntimeConfig,
        control_listener: LinuxControlSocket,
        jobs_listener: LinuxJobsSocket,
    ) -> Result<Self, LinuxRuntimeSetupError> {
        let mut epoll = LinuxEpoll::create().map_err(LinuxRuntimeSetupError::Epoll)?;
        let signal =
            LinuxPid1SignalFd::setup_registered(&epoll, RuntimeEventSource::Pid1Signal.token())
                .map_err(LinuxRuntimeSetupError::Signal)?;
        // Stamped, like the Phase 1 bind in `init::linux::registryd`. `bind`
        // unlinks a stale path, so this replaces the socket registryd bound --
        // and a descriptor applied only there is a descriptor the running
        // system does not have. That is exactly what the first boot with a
        // notify descriptor produced: the directory carried it and the socket
        // did not.
        let notify_socket = NotifySocket::bind_secured(&config.notify_socket_path, |path| {
            crate::boundary::set_path_security(path, crate::notify::NOTIFY_SOCKET_SDDL)
        })
        .map_err(LinuxRuntimeSetupError::NotifySocket)?;
        epoll
            .register_source(
                control_listener.as_raw_fd(),
                RuntimeEventSource::ControlListener,
            )
            .map_err(LinuxRuntimeSetupError::Register)?;
        epoll
            .register_source(notify_socket.as_raw_fd(), RuntimeEventSource::NotifySocket)
            .map_err(LinuxRuntimeSetupError::Register)?;
        epoll
            .register_source(jobs_listener.as_raw_fd(), RuntimeEventSource::JobsListener)
            .map_err(LinuxRuntimeSetupError::Register)?;
        let deadline_timer =
            LinuxTimerFd::create_monotonic().map_err(LinuxRuntimeSetupError::Timer)?;
        epoll
            .register_source(
                deadline_timer.as_raw_fd(),
                RuntimeEventSource::ShutdownDeadlineTimer,
            )
            .map_err(LinuxRuntimeSetupError::Register)?;
        let lifecycle_timer =
            LinuxTimerFd::create_monotonic().map_err(LinuxRuntimeSetupError::Timer)?;
        epoll
            .register_source(
                lifecycle_timer.as_raw_fd(),
                RuntimeEventSource::LifecycleDeadlineTimer,
            )
            .map_err(LinuxRuntimeSetupError::Register)?;
        let power_buttons = setup_power_button_devices(&mut epoll);
        #[cfg(feature = "peios-registry")]
        let registry_watches = setup_registry_watches(&mut epoll)?;

        Ok(Self {
            epoll,
            signal,
            child_reaper: LinuxChildReaper::new(),
            notify_socket,
            control_listener,
            control_connections: ControlConnectionTable::<
                ControlConnectionRecord<LinuxControlConnection>,
            >::new(config.max_control_connections),
            deadline_timer,
            lifecycle_timer,
            calendar_timers: super::calendar_timer::LinuxCalendarTimerTable::new(),
            kmes_sink: crate::boundary::LinuxKmesEventSink::new(),
            console_sink: LinuxConsoleSink::new(),
            quiet: config.quiet,
            quiet_policy: crate::runtime::console::QuietPolicy::new(config.quiet, false),
            log_pipes: RuntimeServiceLogPipes::default(),
            jobs_channel: RuntimeJobsChannelTable::new(jobs_listener, config.max_jobs_connections),
            jobs_connection_timeout_secs: crate::jobs::socket::DEFAULT_JOBS_CONNECTION_TIMEOUT_SECS,
            job_identity_provider: LinuxJobIdentityProvider::new(),
            power_buttons,
            clock: LinuxMonotonicClock::new(),
            controller: LinuxProcessController::new(),
            boot_attempt_counter: LinuxBootAttemptCounter::new(),
            token_provider: LinuxSystemTokenProvider::new(),
            process_launcher: LinuxProcessLauncher::new(),
            filesystem_check_launcher: LinuxFilesystemCheckHelper::new(),
            timer_last_run_writes: super::timer_last_run::TimerLastRunWrites::default(),
            filesystem_check_reader: LinuxFilesystemCheckHelper::new(),
            finalizer: LinuxShutdownFinalizer::new(),
            access_checker: PeiosSystemAccessChecker::new(),
            #[cfg(feature = "peios-registry")]
            registry: LcsRegistryClient,
            #[cfg(feature = "peios-registry")]
            registry_watches,
            config,
        })
    }
}

fn setup_power_button_devices(
    registrar: &mut impl RuntimeEventRegistrar,
) -> crate::boundary::LinuxPowerButtonDevices {
    let mut devices = crate::boundary::LinuxPowerButtonDevices::open_default();
    devices.retain_fds(|fd| {
        registrar
            .register_source(fd, RuntimeEventSource::PowerButton { fd })
            .is_ok()
    });
    devices
}

#[cfg(feature = "peios-registry")]
fn setup_registry_watches(
    registrar: &mut impl RuntimeEventRegistrar,
) -> Result<LcsRegistryWatches, LinuxRuntimeSetupError> {
    let mut watches = LcsRegistryWatches::default();
    for root in [
        crate::boundary::RegistryWatchRoot::Services,
        crate::boundary::RegistryWatchRoot::Init,
    ] {
        let watch =
            LcsRegistryWatch::open_armed(root).map_err(LinuxRuntimeSetupError::RegistryWatch)?;
        let fd = watch.fd();
        registrar
            .register_source(fd, RuntimeEventSource::RegistryWatch { fd })
            .map_err(LinuxRuntimeSetupError::Register)?;
        watches.push(watch);
    }
    Ok(watches)
}
