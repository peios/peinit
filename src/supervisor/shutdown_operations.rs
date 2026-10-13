//! Operation records observe shutdown; only its waves authorise signalling.

use crate::ids::OperationId;
use crate::operation::store::OperationEvent;
use crate::operation::{OperationSource, OperationState};
use crate::service::runtime::ServiceState;
use crate::shutdown::{ShutdownError, ShutdownPlan};

use super::state::SupervisorError;
use super::work::SupervisorWork;

pub(super) fn request_stops(
    work: &mut SupervisorWork,
    plan: &ShutdownPlan,
    now_ns: u64,
) -> Result<Vec<OperationEvent>, SupervisorError> {
    let participants = plan.stop_waves.iter().flat_map(|wave| &wave.services);
    let ids = work
        .operation_ids
        .allocate_batch(participants.clone().count(), now_ns)
        .map_err(SupervisorError::RequestIdAllocation)?;
    let mut events = Vec::new();
    for (participant, id) in participants.zip(ids) {
        events.extend(
            work.operations
                .observe_shutdown_stop(id, participant.service.clone(), now_ns)
                .map_err(|error| SupervisorError::Shutdown(ShutdownError::OperationStore(error)))?
                .events,
        );
    }
    Ok(events)
}

pub(super) fn active_stop(work: &SupervisorWork, service: &str) -> Option<OperationId> {
    work.operations
        .active_for_service(service)
        .into_iter()
        .find(|id| {
            work.operations
                .get(*id)
                .is_some_and(|operation| operation.source == OperationSource::Shutdown)
        })
}

pub(super) fn start_stop(
    work: &mut SupervisorWork,
    service: &str,
    now_ns: u64,
    events: &mut Vec<OperationEvent>,
) -> Result<(), ShutdownError> {
    // A later-wave observation may already have expired (and even been
    // purged). The stop obligation survives it; do not recreate its record.
    if let Some(id) = active_stop(work, service)
        && work
            .operations
            .get(id)
            .is_some_and(|op| op.state == OperationState::Pending)
    {
        events.push(
            work.operations
                .start_operation(id, now_ns)
                .map_err(ShutdownError::OperationStore)?,
        );
    }
    Ok(())
}

pub(super) fn fail_stop(
    work: &mut SupervisorWork,
    service: &str,
    now_ns: u64,
    reason: &str,
    events: &mut Vec<OperationEvent>,
) -> Result<(), ShutdownError> {
    if let Some(id) = active_stop(work, service) {
        events.push(
            work.operations
                .fail_operation(id, now_ns, reason)
                .map_err(ShutdownError::OperationStore)?,
        );
    }
    Ok(())
}

pub(super) fn settle_stopped(
    work: &mut SupervisorWork,
    now_ns: u64,
    events: &mut Vec<OperationEvent>,
) -> Result<(), ShutdownError> {
    let observations: Vec<_> = work
        .operations
        .active_records()
        .into_iter()
        .filter(|op| op.source == OperationSource::Shutdown)
        .map(|op| (op.id, op.service.clone()))
        .collect();
    for (id, service) in observations {
        let state = work.services.runtime(&service).map(|runtime| runtime.state);
        let event = match state {
            None | Some(ServiceState::Inactive | ServiceState::Skipped) => {
                work.operations.complete_operation(id, now_ns, "inactive")
            }
            Some(ServiceState::Failed | ServiceState::Abandoned) => {
                work.operations
                    .fail_operation(id, now_ns, "service failed during shutdown")
            }
            _ => continue,
        };
        events.push(event.map_err(ShutdownError::OperationStore)?);
    }
    Ok(())
}
