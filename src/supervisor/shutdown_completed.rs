use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::shutdown::ShutdownPlan;

use super::work::SupervisorWork;

pub(super) fn clear_completed_services(
    work: &mut SupervisorWork,
    plan: &ShutdownPlan,
) -> Result<Vec<crate::service::ServiceTableTransition>, crate::service::ServiceTableError> {
    plan.completed_to_clear
        .iter()
        .map(|service| {
            work.services.transition_service(
                service,
                ServiceTransition {
                    to: ServiceState::Inactive,
                    cause: TransitionCause::ShutdownWave,
                },
            )
        })
        .collect()
}
