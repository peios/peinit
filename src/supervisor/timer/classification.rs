use crate::service::runtime::ServiceState;
use crate::service::{ServiceDefinition, ServiceType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimerFiringAction {
    Start,
    PendingOneshot,
    SimpleNoop,
    StateNoop { state: ServiceState },
    Disabled,
}

pub(super) fn classify_timer_firing(
    definition: &ServiceDefinition,
    state: ServiceState,
) -> TimerFiringAction {
    match (definition.service_type, state) {
        (
            ServiceType::Oneshot,
            ServiceState::Inactive | ServiceState::Completed | ServiceState::Failed,
        )
        | (ServiceType::Simple, ServiceState::Inactive | ServiceState::Failed) => {
            TimerFiringAction::Start
        }
        (ServiceType::Oneshot, ServiceState::Active | ServiceState::Starting) => {
            TimerFiringAction::PendingOneshot
        }
        (ServiceType::Simple, ServiceState::Active | ServiceState::Starting) => {
            TimerFiringAction::SimpleNoop
        }
        (_, state) => TimerFiringAction::StateNoop { state },
    }
}
