use crate::service::ErrorControl;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::dispatch::SupervisorHealthCheckTerminalDispatch;
use crate::supervisor::work::SupervisorWork;

pub(in crate::supervisor) fn health_critical_reboot_due(
    work: &SupervisorWork,
    dispatch: &SupervisorHealthCheckTerminalDispatch,
) -> bool {
    let Some(service) = dispatch.job_event.service.as_deref() else {
        return false;
    };
    let Some(definition) = work.services.definition(service) else {
        return false;
    };
    definition.error_control == ErrorControl::Critical
        && dispatch.service_transitions.iter().any(|transition| {
            transition.event.to == ServiceState::Failed
                && transition.event.cause == TransitionCause::RestartBudgetExhausted
        })
}
