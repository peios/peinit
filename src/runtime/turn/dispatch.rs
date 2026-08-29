use crate::boundary::{ChildReaper, Clock, ProcessController, RealtimeClock, RegistryClient};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable, ControlListener,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventSource, RuntimeLifecycleDeadlineTimer, RuntimeNotifySource,
    RuntimePid1SignalSource, RuntimeRegistryWatchTurn, RuntimeShutdownDeadlineTimer,
    RuntimeShutdownEventContext, RuntimeShutdownEventSources, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};
use crate::supervisor::Supervisor;

use super::control_connection::process_control_connection_event;
use super::control_listener::process_control_listener_event;
use super::deadline::process_shutdown_deadline_timer_event;
use super::event_sources::NoRuntimeRegistryClient;
use super::lifecycle_deadline::process_lifecycle_deadline_timer_event;
use super::notify::process_notify_event;
use super::power_button::process_power_button_event;
use super::pre_start_check::{
    process_filesystem_check_helper_event, process_filesystem_check_helper_exit_event,
};
use super::process_setup::process_process_setup_event;
use super::signal::process_pid1_signal_event;

pub(crate) fn process_runtime_control_connection_event<I, D, C, P, F, A, R, G>(
    supervisor: &mut Supervisor,
    fd: i32,
    control_connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
    registry: Option<&mut G>,
    deadline_timer: &mut D,
    context: RuntimeShutdownEventContext<'_, C, P, F, A, R>,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    I: ControlConnectionIo,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: crate::boundary::ShutdownFinalizer,
    A: SystemAccessChecker + ServiceAccessChecker + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    G: RegistryClient,
{
    process_control_connection_event(
        supervisor,
        fd,
        control_connections,
        registry,
        deadline_timer,
        context,
    )
}

pub fn process_runtime_shutdown_event<I, L, S, H, N, D, T, C, P, F, A, R>(
    supervisor: &mut Supervisor,
    source: RuntimeEventSource,
    sources: &mut RuntimeShutdownEventSources<'_, I, L, S, H, N, D, T>,
    context: RuntimeShutdownEventContext<'_, C, P, F, A, R>,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    I: ControlConnectionIo,
    L: ControlListener<Connection = I> + ?Sized,
    S: RuntimePid1SignalSource + ?Sized,
    H: ChildReaper + ?Sized,
    N: RuntimeNotifySource + ?Sized,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    T: RuntimeLifecycleDeadlineTimer + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: crate::boundary::ShutdownFinalizer,
    A: SystemAccessChecker + ServiceAccessChecker + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    match source {
        RuntimeEventSource::Pid1Signal => process_pid1_signal_event(
            supervisor,
            &mut *sources.signal_source,
            &mut *sources.child_reaper,
            &mut *sources.deadline_timer,
            context,
        ),
        RuntimeEventSource::ControlListener => {
            let accepted_at_ns = context
                .clock
                .monotonic_ns()
                .map_err(RuntimeShutdownEventTurnError::Clock)?;
            process_control_listener_event(
                &mut *sources.control_listener,
                &mut *sources.control_connections,
                context.registrar,
                Some(accepted_at_ns),
            )
        }
        RuntimeEventSource::ControlConnection { fd } => process_control_connection_event(
            supervisor,
            fd,
            &mut *sources.control_connections,
            None::<&mut NoRuntimeRegistryClient>,
            &mut *sources.deadline_timer,
            context,
        ),
        RuntimeEventSource::ShutdownDeadlineTimer => process_shutdown_deadline_timer_event(
            supervisor,
            &mut *sources.deadline_timer,
            context.clock,
            context.controller,
            context.finalizer,
        ),
        RuntimeEventSource::LifecycleDeadlineTimer => process_lifecycle_deadline_timer_event(
            supervisor,
            &mut *sources.lifecycle_timer,
            &mut *sources.filesystem_check_reader,
            context,
        ),
        RuntimeEventSource::NotifySocket => process_notify_event(
            supervisor,
            &mut *sources.notify_source,
            &mut *sources.deadline_timer,
            context.clock,
            context.controller,
        ),
        RuntimeEventSource::ServiceLogPipe { fd } => Ok(RuntimeShutdownEventTurn::ServiceLogPipe {
            pipe: sources
                .log_pipes
                .process_pipe_event(fd, context.clock, context.registrar),
        }),
        RuntimeEventSource::CalendarTimer { fd } => Ok(RuntimeShutdownEventTurn::CalendarTimer {
            fd,
            turn: crate::runtime::turn::model::RuntimeCalendarTimerTurn::NoRuntimeTable { fd },
        }),
        RuntimeEventSource::FilesystemCheckHelper { result_fd } => {
            process_filesystem_check_helper_event(
                supervisor,
                result_fd,
                &mut *sources.filesystem_check_reader,
                context.clock,
                context.registrar,
            )
        }
        RuntimeEventSource::FilesystemCheckHelperExit { pidfd } => {
            process_filesystem_check_helper_exit_event(
                supervisor,
                pidfd,
                &mut *sources.filesystem_check_reader,
                context.clock,
                context.registrar,
            )
        }
        RuntimeEventSource::RegistryWatch { fd } => Ok(RuntimeShutdownEventTurn::RegistryWatch {
            fd,
            turn: RuntimeRegistryWatchTurn::Unavailable {
                fd,
                reason: "runtime registry watch source unavailable".to_string(),
            },
        }),
        RuntimeEventSource::ProcessSetup { fd } => {
            let Some(process_launcher) = context.process_launcher else {
                return Err(RuntimeShutdownEventTurnError::Supervisor(
                    crate::supervisor::SupervisorError::Launch(
                        crate::execution::launch::LaunchCreatedJobError::Boundary(
                            crate::boundary::BoundaryError::Process(
                                "process setup source has no launcher".to_string(),
                            ),
                        ),
                    ),
                ));
            };
            process_process_setup_event(
                supervisor,
                fd,
                process_launcher,
                context.clock,
                context.controller,
                context.registrar,
                &mut *sources.log_pipes,
            )
        }
        RuntimeEventSource::PowerButton { fd } => process_power_button_event(
            supervisor,
            fd,
            &mut *sources.power_button_source,
            &mut *sources.deadline_timer,
            context,
        ),
    }
}
