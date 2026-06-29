use crate::execution::start::StartExecutionDispatch;
use crate::operation::OperationSource;
use crate::service::ServiceTableTransition;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::super::operation::queue_relationship_stop;
use super::super::start::dispatch_binds_to_recovery_start;

pub(super) fn apply_binds_to_propagation(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
    observed_at_ns: u64,
) -> Result<(), SupervisorError> {
    for transition in transitions {
        let event = &transition.event;
        if !event.from.satisfies_dependents() || event.to.satisfies_dependents() {
            continue;
        }
        for dependent in binds_to_dependents(work, &event.service) {
            if active_dependent_for_binds_to_stop(work, &dependent) {
                queue_relationship_stop(
                    work,
                    &dependent,
                    OperationSource::BindsToPropagation,
                    observed_at_ns,
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn apply_binds_to_recovery_starts(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    let mut dispatches = Vec::new();
    for transition in transitions {
        let event = &transition.event;
        if event.to != ServiceState::Active || event.from.satisfies_dependents() {
            continue;
        }
        for dependent in binds_to_dependents(work, &event.service) {
            if failed_due_to_binds_to_propagation(work, &dependent) {
                dispatches.extend(dispatch_binds_to_recovery_start(
                    work,
                    &dependent,
                    observed_at_ns,
                    max_parallel_starts,
                )?);
            }
        }
    }
    Ok(dispatches)
}

fn binds_to_dependents(work: &SupervisorWork, target: &str) -> Vec<String> {
    work.services
        .service_names()
        .into_iter()
        .filter(|service| *service != target)
        .filter_map(|service| {
            let definition = work.services.definition(service)?;
            definition
                .binds_to
                .iter()
                .any(|binds_to| binds_to == target)
                .then(|| service.to_string())
        })
        .collect()
}

fn active_dependent_for_binds_to_stop(work: &SupervisorWork, service: &str) -> bool {
    work.services.runtime(service).is_some_and(|runtime| {
        matches!(
            runtime.state,
            ServiceState::Active | ServiceState::Reloading
        )
    })
}

fn failed_due_to_binds_to_propagation(work: &SupervisorWork, service: &str) -> bool {
    work.services.runtime(service).is_some_and(|runtime| {
        runtime.state == ServiceState::Failed
            && runtime.cause == Some(TransitionCause::BindsToPropagation)
    })
}
