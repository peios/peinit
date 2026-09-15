use crate::boundary::{Clock, LinuxTimerFdRead, ProcessController, ShutdownDeadlineTimer};
use crate::supervisor::{Supervisor, SupervisorError, SupervisorShutdownDeadlineTimerTurn};

use super::model::{
    RuntimeShutdownDeadlineTimer, RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError,
};

pub(super) fn process_shutdown_deadline_timer_event<D, C, P>(
    supervisor: &mut Supervisor,
    deadline_timer: &mut D,
    clock: &mut C,
    controller: &mut P,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
{
    let read = deadline_timer
        .read_shutdown_deadline()
        .map_err(RuntimeShutdownEventTurnError::DeadlineRead)?;
    let drive = match read {
        LinuxTimerFdRead::Expired { .. } => {
            let now_ns = clock.monotonic_ns().map_err(|error| {
                RuntimeShutdownEventTurnError::Supervisor(SupervisorError::Clock(error))
            })?;
            // The timeouts only. A final action that has come due — the
            // shutdown ready, or a failed action's retry — is taken at the
            // end of the turn, after the console has been written (PEI-827).
            supervisor
                .drive_shutdown(controller, None, now_ns)
                .map_err(RuntimeShutdownEventTurnError::Supervisor)?
                .map(Box::new)
        }
        LinuxTimerFdRead::Canceled | LinuxTimerFdRead::WouldBlock => None,
    };
    let deadline_timer_turn = sync_deadline_timer(supervisor, deadline_timer)?;

    Ok(RuntimeShutdownEventTurn::ShutdownDeadlineTimer {
        read,
        drive,
        deadline_timer: deadline_timer_turn,
    })
}

pub(super) fn sync_deadline_timer<D>(
    supervisor: &Supervisor,
    deadline_timer: &mut D,
) -> Result<SupervisorShutdownDeadlineTimerTurn, RuntimeShutdownEventTurnError>
where
    D: ShutdownDeadlineTimer + ?Sized,
{
    supervisor
        .sync_shutdown_deadline_timer(deadline_timer)
        .map_err(RuntimeShutdownEventTurnError::Supervisor)
}
