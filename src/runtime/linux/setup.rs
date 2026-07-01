use crate::boundary::{
    LinuxBootAttemptCounter, LinuxChildReaper, LinuxConsoleSink, LinuxEpoll,
    LinuxFilesystemCheckHelper, LinuxMonotonicClock, LinuxPid1SignalFd, LinuxProcessController,
    LinuxProcessLauncher, LinuxSystemTokenProvider, LinuxTimerFd,
};
use crate::control::connection::{ControlConnectionRecord, ControlConnectionTable};
use crate::control::socket::{LinuxControlConnection, LinuxControlSocket};
use crate::control::system::PeiosSystemAccessChecker;
use crate::notify::NotifySocket;
#[cfg(feature = "peios-registry")]
use crate::registry::LcsRegistryClient;
#[cfg(feature = "peios-registry")]
use crate::registry::{LcsRegistryWatch, LcsRegistryWatches};
use crate::runtime::{RuntimeEventRegistrar, RuntimeEventSource, RuntimeServiceLogPipes};
use crate::shutdown::LinuxShutdownFinalizer;

use super::{LinuxRuntimeConfig, LinuxRuntimeSetupError, LinuxShutdownRuntime};

impl LinuxShutdownRuntime {
    pub fn setup(config: LinuxRuntimeConfig) -> Result<Self, LinuxRuntimeSetupError> {
        let control_listener = LinuxControlSocket::bind(&config.control_socket_path)
            .map_err(LinuxRuntimeSetupError::ControlSocket)?;
        Self::setup_with_control_listener(config, control_listener)
    }

    pub fn setup_with_infrastructure(
        config: LinuxRuntimeConfig,
        infrastructure: &mut crate::init::Phase1Infrastructure,
    ) -> Result<Self, LinuxRuntimeSetupError> {
        let (runtime, _) = Self::setup_with_infrastructure_registration(config, infrastructure)?;
        Ok(runtime)
    }

    pub fn setup_with_infrastructure_registration(
        config: LinuxRuntimeConfig,
        infrastructure: &mut crate::init::Phase1Infrastructure,
    ) -> Result<(Self, crate::runtime::Phase1InfrastructureRegistration), LinuxRuntimeSetupError>
    {
        let control_listener = infrastructure
            .take_control_socket()
            .ok_or(LinuxRuntimeSetupError::MissingControlSocket)?;
        let mut runtime = Self::setup_with_control_listener(config, control_listener)?;
        let registration = runtime.register_phase1_infrastructure(infrastructure);
        Ok((runtime, registration))
    }

    fn setup_with_control_listener(
        config: LinuxRuntimeConfig,
        control_listener: LinuxControlSocket,
    ) -> Result<Self, LinuxRuntimeSetupError> {
        let mut epoll = LinuxEpoll::create().map_err(LinuxRuntimeSetupError::Epoll)?;
        let signal =
            LinuxPid1SignalFd::setup_registered(&epoll, RuntimeEventSource::Pid1Signal.token())
                .map_err(LinuxRuntimeSetupError::Signal)?;
        let notify_socket = NotifySocket::bind(&config.notify_socket_path)
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
            log_pipes: RuntimeServiceLogPipes::default(),
            jfs_device: None,
            power_buttons,
            clock: LinuxMonotonicClock::new(),
            controller: LinuxProcessController::new(),
            boot_attempt_counter: LinuxBootAttemptCounter::new(),
            token_provider: LinuxSystemTokenProvider::new(),
            process_launcher: LinuxProcessLauncher::new(),
            filesystem_check_launcher: LinuxFilesystemCheckHelper::new(),
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
