use crate::execution::job_terminal::ServiceMainJobTerminalDispatch;
use crate::service::ErrorControl;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::work::SupervisorWork;

pub(super) fn critical_reboot_due(
    work: &SupervisorWork,
    dispatch: &ServiceMainJobTerminalDispatch,
) -> bool {
    if work.shutdown.is_some() {
        return false;
    }
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
