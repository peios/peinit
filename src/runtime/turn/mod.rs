//! Runtime event adapters.
//!
//! The runtime layer owns Linux/event-loop objects and translates readable
//! sources into supervisor turns. It may call `Supervisor` directly, but it
//! should not encode service-manager policy beyond event ordering, source
//! registration, and fail-soft handling for optional producers such as eventd
//! and JFS.

mod control_connection;
mod control_listener;
mod deadline;
mod dispatch;
mod event_sources;
mod jfs;
mod lifecycle_deadline;
mod model;
mod notify;
mod power_button;
mod pre_start_check;
#[cfg(test)]
mod pre_start_check_tests;
mod process_setup;
mod registry_watch;
mod signal;

pub(crate) use dispatch::process_runtime_control_connection_event;
pub use dispatch::process_runtime_shutdown_event;
pub(crate) use event_sources::NoRuntimeRegistryClient;
pub use event_sources::RuntimeShutdownEventSources;
pub use model::{
    RuntimeCalendarTimerTurn, RuntimeEventRegistrar, RuntimeEventRegistrationError,
    RuntimeFilesystemCheckHelperTurn, RuntimeJfsDeviceTurn, RuntimeLifecycleDeadlineTimer,
    RuntimeNotifyDatagram, RuntimeNotifyRead, RuntimeNotifyRejection, RuntimeNotifySource,
    RuntimeNotifySupervisorTurn, RuntimePid1SignalSource, RuntimePowerButtonSource,
    RuntimePowerButtonTurn, RuntimeProcessSetupTurn, RuntimeRegistryWatchTurn,
    RuntimeShutdownDeadlineTimer, RuntimeShutdownEventContext, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};
pub(crate) use pre_start_check::register_filesystem_check_helper_sources;
pub(crate) use process_setup::register_process_setup_sources;
pub(crate) use registry_watch::process_registry_watch_event;
