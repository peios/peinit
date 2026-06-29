use crate::boundary::{
    BootAttemptCounter, Clock, LinuxTimerFdRead, ProcessController, ShutdownDeadlineTimer,
    ShutdownFinalizer,
};
use crate::supervisor::{Supervisor, SupervisorError, SupervisorLifecycleDeadlineTimerTurn};

use super::model::{
    RuntimeEventRegistrar, RuntimeLifecycleDeadlineTimer, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};

pub(super) fn process_lifecycle_deadline_timer_event<D, C, P, F, R>(
    supervisor: &mut Supervisor,
    deadline_timer: &mut D,
    clock: &mut C,
    controller: &mut P,
    boot_attempt_counter: &mut dyn BootAttemptCounter,
    finalizer: &mut F,
    registrar: &mut R,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    D: RuntimeLifecycleDeadlineTimer + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    R: RuntimeEventRegistrar + ?Sized,
{
    let read = deadline_timer
        .read_lifecycle_deadline()
        .map_err(RuntimeShutdownEventTurnError::DeadlineRead)?;
    let drive = match read {
        LinuxTimerFdRead::Expired { .. } => {
            let now_ns = clock.monotonic_ns().map_err(|error| {
                RuntimeShutdownEventTurnError::Supervisor(SupervisorError::Clock(error))
            })?;
            let dispatch = supervisor
                .process_due_lifecycle_deadlines_with_finalizer(
                    controller,
                    boot_attempt_counter,
                    Some(finalizer),
                    now_ns,
                )
                .map_err(RuntimeShutdownEventTurnError::Supervisor)?;
            if let Some(dispatch) = &dispatch {
                unregister_filesystem_check_timeouts(dispatch, registrar)?;
            }
            dispatch.map(Box::new)
        }
        LinuxTimerFdRead::Canceled | LinuxTimerFdRead::WouldBlock => None,
    };
    let deadline_timer_turn = sync_lifecycle_deadline_timer(supervisor, deadline_timer)?;

    Ok(RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        read,
        drive,
        deadline_timer: deadline_timer_turn,
    })
}

fn unregister_filesystem_check_timeouts<R>(
    dispatch: &crate::supervisor::SupervisorLifecycleDeadlineDispatch,
    registrar: &mut R,
) -> Result<(), RuntimeShutdownEventTurnError>
where
    R: RuntimeEventRegistrar + ?Sized,
{
    for timeout in &dispatch.pre_start_check_timeouts {
        registrar
            .unregister_source(timeout.timeout.completion.result_fd)
            .map_err(RuntimeShutdownEventTurnError::EventRegistration)?;
        registrar
            .unregister_source(timeout.timeout.pidfd)
            .map_err(RuntimeShutdownEventTurnError::EventRegistration)?;
    }
    Ok(())
}

pub(super) fn sync_lifecycle_deadline_timer<D>(
    supervisor: &Supervisor,
    deadline_timer: &mut D,
) -> Result<SupervisorLifecycleDeadlineTimerTurn, RuntimeShutdownEventTurnError>
where
    D: ShutdownDeadlineTimer + ?Sized,
{
    supervisor
        .sync_lifecycle_deadline_timer(deadline_timer)
        .map_err(RuntimeShutdownEventTurnError::Supervisor)
}
