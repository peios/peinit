use crate::boundary::{
    ChildReap, ChildReaper, Clock, LinuxSignalFdRead, ProcessController, RealtimeClock,
    ShutdownFinalizer,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::{Supervisor, SupervisorChildReapDispatch, SupervisorPid1SignalFdTurn};

use super::deadline::sync_deadline_timer;
use super::model::{
    RuntimeEventRegistrar, RuntimePid1SignalSource, RuntimeShutdownDeadlineTimer,
    RuntimeShutdownEventContext, RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError,
};

pub(super) fn process_pid1_signal_event<S, H, D, C, P, F, A, R>(
    supervisor: &mut Supervisor,
    signal_source: &mut S,
    child_reaper: &mut H,
    deadline_timer: &mut D,
    context: RuntimeShutdownEventContext<'_, C, P, F, A, R>,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    S: RuntimePid1SignalSource + ?Sized,
    H: ChildReaper + ?Sized,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    A: SystemAccessChecker + ServiceAccessChecker + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    let read = signal_source
        .read_pid1_signal()
        .map_err(RuntimeShutdownEventTurnError::SignalRead)?;
    let supervisor_turn = supervisor
        .handle_pid1_signal_fd_read(read, context.clock, context.controller, context.finalizer)
        .map_err(RuntimeShutdownEventTurnError::Supervisor)?;
    let child_reaps = if matches!(read, LinuxSignalFdRead::Other { signal } if signal == libc::SIGCHLD)
    {
        process_sigchld_reaps(
            supervisor,
            child_reaper,
            context.clock,
            context.controller,
            context.finalizer,
        )?
    } else {
        Vec::new()
    };
    let deadline_timer_turn = if matches!(supervisor_turn, SupervisorPid1SignalFdTurn::Shutdown(_))
        || child_reaps_advanced_shutdown(&child_reaps)
    {
        Some(sync_deadline_timer(supervisor, deadline_timer)?)
    } else {
        None
    };
    Ok(RuntimeShutdownEventTurn::Pid1Signal {
        read,
        supervisor: supervisor_turn,
        child_reaps,
        deadline_timer: deadline_timer_turn,
    })
}

fn process_sigchld_reaps<H, C, P, F>(
    supervisor: &mut Supervisor,
    child_reaper: &mut H,
    clock: &mut C,
    controller: &mut P,
    finalizer: &mut F,
) -> Result<Vec<crate::supervisor::SupervisorChildReapTurn>, RuntimeShutdownEventTurnError>
where
    H: ChildReaper + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
{
    let children = child_reaper
        .reap_children()
        .map_err(RuntimeShutdownEventTurnError::ChildReap)?;
    if children.is_empty() {
        return Ok(Vec::new());
    }
    let ended_at_ns = clock.monotonic_ns().map_err(|error| {
        RuntimeShutdownEventTurnError::Supervisor(crate::supervisor::SupervisorError::Clock(error))
    })?;
    children
        .into_iter()
        .map(|child: ChildReap| {
            supervisor.apply_reaped_child(child, ended_at_ns, controller, finalizer)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(RuntimeShutdownEventTurnError::Supervisor)
}

fn child_reaps_advanced_shutdown(
    child_reaps: &[crate::supervisor::SupervisorChildReapTurn],
) -> bool {
    child_reaps.iter().any(|turn| {
        matches!(
            turn,
            crate::supervisor::SupervisorChildReapTurn::Tracked {
                dispatch: SupervisorChildReapDispatch::Shutdown(_),
                ..
            }
        )
    })
}
