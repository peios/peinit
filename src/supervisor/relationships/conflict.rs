use std::collections::BTreeSet;

use crate::execution::graph::GraphContextId;
use crate::execution::start::StartExecutionDispatch;
use crate::operation::OperationSource;
use crate::service::ServiceTable;
use crate::service::runtime::ServiceState;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::operation::queue_relationship_stop;

pub(in crate::supervisor) fn gate_start_context(
    work: &mut SupervisorWork,
    context_id: GraphContextId,
    start_services: BTreeSet<String>,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    let blockers = active_conflicts_for_start(&work.services, &start_services);
    if blockers.is_empty() {
        return work.release_for_context(context_id, max_parallel_starts, observed_at_ns);
    }

    for blocker in &blockers {
        queue_relationship_stop(
            work,
            blocker,
            OperationSource::ConflictResolution,
            observed_at_ns,
        )?;
    }
    work.relationships
        .record_conflict_context(context_id, start_services, blockers);
    Ok(Vec::new())
}

pub(super) fn active_conflicts_for_start(
    services: &ServiceTable,
    start_services: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut blockers = BTreeSet::new();
    for service in start_services {
        if let Some(definition) = services.definition(service) {
            for target in &definition.conflicts {
                push_active_blocker(services, start_services, target, &mut blockers);
            }
        }
    }

    for candidate in services.service_names() {
        if start_services.contains(candidate) {
            continue;
        }
        let Some(definition) = services.definition(candidate) else {
            continue;
        };
        if definition
            .conflicts
            .iter()
            .any(|target| start_services.contains(target))
        {
            push_active_blocker(services, start_services, candidate, &mut blockers);
        }
    }
    blockers
}

pub(super) fn blocker_still_present(services: &ServiceTable, blocker: &str) -> bool {
    services.runtime(blocker).is_some_and(|runtime| {
        matches!(
            runtime.state,
            ServiceState::Starting
                | ServiceState::Active
                | ServiceState::Reloading
                | ServiceState::Stopping
                | ServiceState::Abandoned
        )
    })
}

fn push_active_blocker(
    services: &ServiceTable,
    start_services: &BTreeSet<String>,
    candidate: &str,
    blockers: &mut BTreeSet<String>,
) {
    if start_services.contains(candidate) {
        return;
    }
    if services.runtime(candidate).is_some_and(|runtime| {
        matches!(
            runtime.state,
            ServiceState::Active | ServiceState::Reloading
        )
    }) {
        blockers.insert(candidate.to_string());
    }
}
