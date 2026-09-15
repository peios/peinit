//! The Critical-budget reboot the runtime raises once per turn.
//!
//! The supervisor's reconciliation pass finalises the reboot; this adapter
//! adds what every other path that finalises already does afterwards, which
//! is to re-sync the shutdown deadline timer. Without that a `reboot(2)` that
//! returned left the shutdown in its Failed state with `next_retry_at_ns`
//! never armed, so nothing ever retried it (PEI-1087).

use crate::boundary::{ShutdownDeadlineTimer, ShutdownFinalizer};
use crate::supervisor::{
    Supervisor, SupervisorCriticalBudgetRebootDispatch, SupervisorError,
    SupervisorShutdownDeadlineTimerTurn,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCriticalBudgetRebootTurn {
    pub dispatch: SupervisorCriticalBudgetRebootDispatch,
    pub deadline_timer: SupervisorShutdownDeadlineTimerTurn,
}

/// Reboot if a Critical service has run out of restart budget, and arm the
/// retry of a final action that failed.
pub fn process_due_critical_budget_reboot<F, D>(
    supervisor: &mut Supervisor,
    finalizer: &mut F,
    deadline_timer: &mut D,
    now_ns: u64,
) -> Result<Option<RuntimeCriticalBudgetRebootTurn>, SupervisorError>
where
    F: ShutdownFinalizer + ?Sized,
    D: ShutdownDeadlineTimer + ?Sized,
{
    let Some(dispatch) = supervisor.process_due_critical_budget_reboot(finalizer, now_ns)? else {
        return Ok(None);
    };
    let deadline_timer = supervisor.sync_shutdown_deadline_timer(deadline_timer)?;
    Ok(Some(RuntimeCriticalBudgetRebootTurn {
        dispatch,
        deadline_timer,
    }))
}
