use crate::boundary::ShutdownFinalizer;
use crate::service::ErrorControl;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::SupervisorLifecycleDeadlineDispatch;
use crate::supervisor::critical_budget::CriticalRebootTrigger;
use crate::supervisor::dispatch::SupervisorWatchdogTimeoutDispatch;
use crate::supervisor::state::{Supervisor, SupervisorError};

/// Raise the Critical reboot a deadline in this turn has earned, or — given
/// no finalizer — leave it to the reconciliation pass with its cause noted.
pub(super) fn annotate_critical_reboot_if_due(
    supervisor: &mut Supervisor,
    dispatch: &mut SupervisorLifecycleDeadlineDispatch,
    finalizer: Option<&mut dyn ShutdownFinalizer>,
    now_ns: u64,
) -> Result<(), SupervisorError> {
    if let Some(index) = health_timeout_critical_reboot_index(supervisor, dispatch) {
        match finalizer {
            Some(finalizer) => {
                dispatch.health_check_timeouts[index]
                    .terminal
                    .critical_reboot = Some(supervisor.critical_reboot(finalizer, now_ns)?);
            }
            None => {
                if let Some(service) = dispatch.health_check_timeouts[index]
                    .terminal
                    .job_event
                    .service
                    .as_deref()
                {
                    supervisor.note_deferred_critical_reboot(
                        service,
                        CriticalRebootTrigger::HealthCheckFailure,
                        Some(now_ns),
                    );
                }
            }
        }
    } else if let Some(index) = watchdog_timeout_critical_reboot_index(supervisor, dispatch) {
        match finalizer {
            Some(finalizer) => {
                dispatch.watchdog_timeouts[index].critical_reboot =
                    Some(supervisor.critical_reboot(finalizer, now_ns)?);
            }
            None => {
                let service = dispatch.watchdog_timeouts[index].service.clone();
                supervisor.note_deferred_critical_reboot(
                    &service,
                    CriticalRebootTrigger::WatchdogTimeout,
                    Some(now_ns),
                );
            }
        }
    } else if let Some(finalizer) = finalizer {
        // Every other way a Critical service can have exhausted its budget in
        // this turn: a readiness timeout, a pre-start hook or check timeout.
        // Asked as one question about the service rather than list by list —
        // enumerating the deadlines that can cause a reboot is exactly what
        // was incomplete before (PEI-341), and a new deadline kind would make
        // it incomplete again. Without a finalizer the same question is asked
        // by the runtime at the end of its turn, and nothing needs noting: the
        // budget itself is the whole story on these paths.
        dispatch.critical_budget_reboot =
            supervisor.process_due_critical_budget_reboot(finalizer, now_ns)?;
    }
    Ok(())
}

fn watchdog_timeout_critical_reboot_index(
    supervisor: &Supervisor,
    dispatch: &SupervisorLifecycleDeadlineDispatch,
) -> Option<usize> {
    dispatch
        .watchdog_timeouts
        .iter()
        .position(|timeout| watchdog_timeout_critical_reboot_due(supervisor, timeout))
}

fn watchdog_timeout_critical_reboot_due(
    supervisor: &Supervisor,
    timeout: &SupervisorWatchdogTimeoutDispatch,
) -> bool {
    let Some(definition) = supervisor.services.definition(&timeout.service) else {
        return false;
    };
    definition.error_control == ErrorControl::Critical
        && timeout.service_transitions.iter().any(|transition| {
            transition.event.to == ServiceState::Failed
                && transition.event.cause == TransitionCause::RestartBudgetExhausted
        })
}

fn health_timeout_critical_reboot_index(
    supervisor: &Supervisor,
    dispatch: &SupervisorLifecycleDeadlineDispatch,
) -> Option<usize> {
    dispatch
        .health_check_timeouts
        .iter()
        .position(|timeout| health_timeout_critical_reboot_due(supervisor, timeout))
}

fn health_timeout_critical_reboot_due(
    supervisor: &Supervisor,
    timeout: &crate::supervisor::dispatch::SupervisorHealthCheckTimeoutDispatch,
) -> bool {
    let Some(service) = timeout.terminal.job_event.service.as_deref() else {
        return false;
    };
    let Some(definition) = supervisor.services.definition(service) else {
        return false;
    };
    definition.error_control == ErrorControl::Critical
        && timeout
            .terminal
            .service_transitions
            .iter()
            .any(|transition| {
                transition.event.to == ServiceState::Failed
                    && transition.event.cause == TransitionCause::RestartBudgetExhausted
            })
}
