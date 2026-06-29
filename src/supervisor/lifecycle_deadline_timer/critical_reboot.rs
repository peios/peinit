use crate::boundary::ShutdownFinalizer;
use crate::service::ErrorControl;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::SupervisorLifecycleDeadlineDispatch;
use crate::supervisor::dispatch::SupervisorWatchdogTimeoutDispatch;
use crate::supervisor::state::{Supervisor, SupervisorError};

pub(super) fn annotate_critical_reboot_if_due<F>(
    supervisor: &mut Supervisor,
    dispatch: &mut SupervisorLifecycleDeadlineDispatch,
    finalizer: &mut F,
    now_ns: u64,
) -> Result<(), SupervisorError>
where
    F: ShutdownFinalizer + ?Sized,
{
    if let Some(index) = health_timeout_critical_reboot_index(supervisor, dispatch) {
        dispatch.health_check_timeouts[index]
            .terminal
            .critical_reboot = Some(supervisor.critical_reboot(finalizer, now_ns)?);
    } else if let Some(index) = watchdog_timeout_critical_reboot_index(supervisor, dispatch) {
        dispatch.watchdog_timeouts[index].critical_reboot =
            Some(supervisor.critical_reboot(finalizer, now_ns)?);
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
