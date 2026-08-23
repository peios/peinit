use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceTable, ServiceTableTransition};

use super::model::StartExecutionError;

/// Clear a Skipped service so an explicit start can re-evaluate its conditions.
///
/// `Skipped -> Starting` is not a permitted edge; `Skipped -> Inactive` on
/// `ExplicitReset | ExplicitStart` is. Until PEI-342's sibling PEI-340 nothing
/// performed the second one, so `svctl start` on a service skipped at boot
/// reached the activation still Skipped and died on `InvalidTransition` — an
/// internal error where the state machine promises a re-evaluation. That is
/// exactly the case the edge exists for: a condition on a path that did not
/// exist yet, a device that has since appeared, a mount that has since been
/// made. The operator fixes the precondition and starts the service.
///
/// This runs before the activation snapshot is taken, for two reasons. The
/// checks themselves transition `-> Skipped` when a condition is still unmet,
/// and `Skipped -> Skipped` is not permitted either, so clearing after the
/// checks would only move the failure. And `transition_service` drains a
/// pending definition when a service reaches a state that does not retain one,
/// so clearing first means the activation runs on the definition the clear
/// installed rather than the one it replaced.
///
/// A start caused by anything else is left alone. A Skipped service satisfies
/// its dependents, so a dependency-caused start never reaches here — only the
/// explicitly requested service does.
pub(super) fn clear_skipped_for_explicit_start(
    services: &mut ServiceTable,
    service: &str,
    cause: TransitionCause,
) -> Result<Option<ServiceTableTransition>, StartExecutionError> {
    if cause != TransitionCause::ExplicitStart {
        return Ok(None);
    }
    if services.runtime(service).map(|runtime| runtime.state) != Some(ServiceState::Skipped) {
        return Ok(None);
    }
    services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Inactive,
                cause,
            },
        )
        .map(Some)
        .map_err(StartExecutionError::ServiceTable)
}
