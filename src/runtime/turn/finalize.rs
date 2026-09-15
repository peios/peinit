//! The final action a runtime turn ends in, taken after everything else the
//! turn has to say.
//!
//! No event handler finalises a shutdown or reboots for a Critical service.
//! They leave the state saying that it is due — Ready, an immediate final
//! action already due its first attempt, a Critical reboot owed — and the
//! runtime runs this once its events have been applied and its console
//! output written. `reboot(2)` does not return on success, so anything the
//! turn would have said afterwards was never heard: "shutdown ready to
//! finalize", "shutdown finalizing", the forced-reboot line and, worst, the
//! name of the Critical service that took the machine down (PEI-827).
//!
//! The deadline timer is re-synced afterwards, as on every other path that
//! changes the shutdown's deadlines: a `reboot(2)` that returned leaves a
//! retry to arm, and without that nothing ever retried it (PEI-1087).

use crate::boundary::{ShutdownDeadlineTimer, ShutdownFinalizer};
use crate::supervisor::{
    CriticalRebootOwed, Supervisor, SupervisorCriticalBudgetRebootDispatch, SupervisorError,
    SupervisorShutdownDeadlineTimerTurn, SupervisorShutdownFinalizationDispatch,
};

/// What [`finalize_due_shutdown`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeShutdownFinalizationTurn {
    /// The reboot raised for a Critical service out of restart budget.
    pub critical_budget_reboot: Option<SupervisorCriticalBudgetRebootDispatch>,
    /// The final action of a shutdown that was ready for it, or a failed
    /// one's retry.
    pub finalization: Option<SupervisorShutdownFinalizationDispatch>,
    pub deadline_timer: SupervisorShutdownDeadlineTimerTurn,
}

/// What [`finalize_due_shutdown`] is about to do, for announcing before it
/// does it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePendingShutdownFinalization {
    CriticalBudgetReboot(CriticalRebootOwed),
    FinalAction,
}

/// What the turn's final action would be if taken now, without taking it.
pub fn pending_shutdown_finalization(
    supervisor: &Supervisor,
    now_ns: u64,
) -> Option<RuntimePendingShutdownFinalization> {
    if let Some(owed) = supervisor.critical_budget_reboot_owed() {
        return Some(RuntimePendingShutdownFinalization::CriticalBudgetReboot(
            owed,
        ));
    }
    supervisor
        .shutdown_finalization_due(now_ns)
        .then_some(RuntimePendingShutdownFinalization::FinalAction)
}

/// Reboot for a Critical service out of restart budget, or run the final
/// action of a shutdown that is due one; arm the retry if it returned.
pub fn finalize_due_shutdown<F, D>(
    supervisor: &mut Supervisor,
    finalizer: &mut F,
    deadline_timer: &mut D,
    now_ns: u64,
) -> Result<Option<RuntimeShutdownFinalizationTurn>, SupervisorError>
where
    F: ShutdownFinalizer + ?Sized,
    D: ShutdownDeadlineTimer + ?Sized,
{
    let critical_budget_reboot =
        supervisor.process_due_critical_budget_reboot(finalizer, now_ns)?;
    let finalization = if critical_budget_reboot.is_some() {
        None
    } else {
        supervisor.finalize_due_shutdown(finalizer, now_ns)?
    };
    if critical_budget_reboot.is_none() && finalization.is_none() {
        return Ok(None);
    }
    let deadline_timer = supervisor.sync_shutdown_deadline_timer(deadline_timer)?;
    Ok(Some(RuntimeShutdownFinalizationTurn {
        critical_budget_reboot,
        finalization,
        deadline_timer,
    }))
}
