use crate::boundary::{
    Clock, FilesystemCheckHelperReader, LinuxTimerFdRead, ProcessController, RealtimeClock,
    ShutdownDeadlineTimer, ShutdownFinalizer,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::{Supervisor, SupervisorError, SupervisorLifecycleDeadlineTimerTurn};

use super::model::{
    RuntimeEventRegistrar, RuntimeLifecycleDeadlineTimer, RuntimeShutdownEventContext,
    RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError,
};

pub(super) fn process_lifecycle_deadline_timer_event<D, C, P, F, A, R, H>(
    supervisor: &mut Supervisor,
    deadline_timer: &mut D,
    filesystem_check_reader: &mut H,
    context: RuntimeShutdownEventContext<'_, C, P, F, A, R>,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    D: RuntimeLifecycleDeadlineTimer + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    A: SystemAccessChecker + ServiceAccessChecker + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    H: FilesystemCheckHelperReader + ?Sized,
{
    let RuntimeShutdownEventContext {
        clock,
        controller,
        finalizer,
        registrar,
        boot_attempt_counter,
        ..
    } = context;
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
                release_filesystem_check_timeouts(dispatch, registrar, filesystem_check_reader)?;
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

/// A timed-out helper is the third and last way a helper leaves the store, so
/// its descriptors are released here for the same reason as on the completion
/// paths -- see `super::pre_start_check::release_helper_fds`.
fn release_filesystem_check_timeouts<R, H>(
    dispatch: &crate::supervisor::SupervisorLifecycleDeadlineDispatch,
    registrar: &mut R,
    reader: &mut H,
) -> Result<(), RuntimeShutdownEventTurnError>
where
    R: RuntimeEventRegistrar + ?Sized,
    H: FilesystemCheckHelperReader + ?Sized,
{
    for timeout in &dispatch.pre_start_check_timeouts {
        super::pre_start_check::release_helper_fds(
            timeout.timeout.completion.result_fd,
            timeout.timeout.pidfd,
            registrar,
            reader,
        )?;
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
