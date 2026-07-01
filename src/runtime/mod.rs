#[cfg(any(test, feature = "peios-boundary"))]
mod console;
mod event_loop;
mod jfs;
#[cfg(feature = "peios-boundary")]
mod kmes;
#[cfg(feature = "peios-boundary")]
mod linux;
mod logging;
mod source;
mod turn;
mod work_pump;

#[cfg(feature = "peios-boundary")]
pub(crate) use console::collect_runtime_loop_console_messages;
#[cfg(feature = "peios-boundary")]
pub(crate) use event_loop::prepare_runtime_shutdown_loop_turn;
#[cfg(any(test, feature = "peios-boundary"))]
pub(crate) use event_loop::process_runtime_shutdown_sources_with_registry;
#[cfg(feature = "peios-boundary")]
pub(crate) use kmes::collect_runtime_loop_kmes_events;
pub(crate) use turn::{register_filesystem_check_helper_sources, register_process_setup_sources};

pub use event_loop::{
    DEFAULT_MAX_CONTROL_READ_BYTES, RuntimeControlLimits, RuntimeEventWaitError,
    RuntimeEventWaiter, RuntimeShutdownLoopContext, RuntimeShutdownLoopError,
    RuntimeShutdownLoopTurn, process_runtime_shutdown_loop_turn,
};
#[cfg(feature = "peios-boundary")]
pub use linux::{
    DEFAULT_MAX_RUNTIME_EVENTS, LinuxRuntimeConfig, LinuxRuntimeSetupError, LinuxShutdownRuntime,
    Phase1InfrastructureRegistration, Phase1JfsRegistration,
};
pub use logging::{
    DEFAULT_LOG_READ_BYTES_PER_EVENT, DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
    DEFAULT_MAX_LOG_LINE_BYTES, RuntimeEventdLogFlush, RuntimeLogConfig, RuntimeLogPipeTurn,
    RuntimeServiceLogPipes,
};
pub use source::{RuntimeEventSource, RuntimeEventSourceDecodeError};
pub(crate) use turn::{
    NoRuntimeRegistryClient, process_registry_watch_event, process_runtime_control_connection_event,
};
pub use turn::{
    RuntimeCalendarTimerTurn, RuntimeEventRegistrar, RuntimeEventRegistrationError,
    RuntimeFilesystemCheckHelperTurn, RuntimeJfsDeviceTurn, RuntimeLifecycleDeadlineTimer,
    RuntimeNotifyDatagram, RuntimeNotifyRead, RuntimeNotifyRejection, RuntimeNotifySource,
    RuntimeNotifySupervisorTurn, RuntimePid1SignalSource, RuntimePowerButtonSource,
    RuntimePowerButtonTurn, RuntimeProcessSetupTurn, RuntimeRegistryWatchTurn,
    RuntimeShutdownDeadlineTimer, RuntimeShutdownEventContext, RuntimeShutdownEventSources,
    RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError, process_runtime_shutdown_event,
};
pub use work_pump::{
    RuntimeWorkPumpConfig, RuntimeWorkPumpContext, RuntimeWorkPumpError, RuntimeWorkPumpTurn,
    drain_runtime_work_queues,
};
