use crate::boundary::{
    ChildReap, ChildReaper, Clock, LinuxSignalFdRead, ProcessController, RealtimeClock,
    ShutdownFinalizer,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::{
    Supervisor, SupervisorChildReapDispatch, SupervisorChildReapTurn, SupervisorPid1SignalFdTurn,
    SupervisorShutdownSignalAction,
};

use super::deadline::sync_deadline_timer;
use super::model::{
    RuntimeEventRegistrar, RuntimePid1SignalSource, RuntimeShutdownDeadlineTimer,
    RuntimeShutdownEventContext, RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError,
};
use super::process_setup::release_cancelled_process_setups;

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
    A: SystemAccessChecker
        + ServiceAccessChecker
        + crate::submitted::JobAccessChecker
        + crate::submitted::JobDescriptorFactory
        + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    let read = signal_source
        .read_pid1_signal()
        .map_err(RuntimeShutdownEventTurnError::SignalRead)?;
    // No finalizer on any of these: a forced reboot, a Critical service's
    // reboot and a shutdown's final action are all left to the end of the
    // turn (`finalize_due_shutdown`), once the turn's console output has
    // been written (PEI-827).
    let supervisor_turn = supervisor
        .handle_pid1_signal_fd_read(read, context.clock, context.controller, None)
        .map_err(RuntimeShutdownEventTurnError::Supervisor)?;
    if let SupervisorPid1SignalFdTurn::Shutdown(dispatch) = &supervisor_turn
        && let SupervisorShutdownSignalAction::Graceful(shutdown) = &dispatch.action
    {
        release_cancelled_process_setups(&shutdown.cancelled_setups, context.registrar);
    }
    let reaps = if matches!(read, LinuxSignalFdRead::Other { signal } if signal == libc::SIGCHLD) {
        process_sigchld_reaps(supervisor, child_reaper, context.clock, context.controller)?
    } else {
        SigchldReaps::empty()
    };
    let shutdown_advanced = child_reaps_advanced_shutdown(&reaps.child_reaps);
    let drive = if shutdown_advanced {
        let now_ns = reaps.ended_at_ns.expect("shutdown reaps have a timestamp");
        supervisor
            .drive_shutdown(context.controller, None, now_ns)
            .map_err(RuntimeShutdownEventTurnError::Supervisor)?
            .map(Box::new)
    } else {
        None
    };
    let deadline_timer_turn = if matches!(supervisor_turn, SupervisorPid1SignalFdTurn::Shutdown(_))
        || shutdown_advanced
    {
        Some(sync_deadline_timer(supervisor, deadline_timer)?)
    } else {
        None
    };
    Ok(RuntimeShutdownEventTurn::Pid1Signal {
        read,
        supervisor: supervisor_turn,
        child_reaps: reaps.child_reaps,
        drive,
        deadline_timer: deadline_timer_turn,
    })
}

struct SigchldReaps {
    child_reaps: Vec<SupervisorChildReapTurn>,
    ended_at_ns: Option<u64>,
}

impl SigchldReaps {
    fn empty() -> Self {
        Self {
            child_reaps: Vec::new(),
            ended_at_ns: None,
        }
    }
}

fn process_sigchld_reaps<H, C, P>(
    supervisor: &mut Supervisor,
    child_reaper: &mut H,
    clock: &mut C,
    controller: &mut P,
) -> Result<SigchldReaps, RuntimeShutdownEventTurnError>
where
    H: ChildReaper + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
{
    let children = child_reaper
        .reap_children()
        .map_err(RuntimeShutdownEventTurnError::ChildReap)?;
    if children.is_empty() {
        return Ok(SigchldReaps::empty());
    }
    let ended_at_ns = clock.monotonic_ns().map_err(|error| {
        RuntimeShutdownEventTurnError::Supervisor(crate::supervisor::SupervisorError::Clock(error))
    })?;
    let child_reaps = children
        .into_iter()
        .map(|child: ChildReap| supervisor.apply_reaped_child(child, ended_at_ns, controller, None))
        .collect::<Result<Vec<_>, _>>()
        .map_err(RuntimeShutdownEventTurnError::Supervisor)?;
    Ok(SigchldReaps {
        child_reaps,
        ended_at_ns: Some(ended_at_ns),
    })
}

fn child_reaps_advanced_shutdown(child_reaps: &[SupervisorChildReapTurn]) -> bool {
    child_reaps.iter().any(|turn| {
        matches!(
            turn,
            SupervisorChildReapTurn::Tracked {
                dispatch: SupervisorChildReapDispatch::Shutdown(_),
                ..
            }
        )
    })
}
