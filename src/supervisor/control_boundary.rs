use std::collections::VecDeque;

use crate::ids::OperationId;
use crate::operation::store::{OperationRequestOutcome, OperationStore};
use crate::operation::{OperationState, OperationType};
use crate::service::ServiceTable;
use crate::service::runtime::ServiceState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingControlOperation {
    pub operation_id: OperationId,
    pub service: String,
    pub operation_type: OperationType,
    pub requirement: PendingControlRequirement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingControlRequirement {
    StopProcess,
    ReloadProcess,
    RestartProcess,
}

pub(super) fn queue_control_boundary(
    queue: &mut VecDeque<PendingControlOperation>,
    operations: &OperationStore,
    services: &ServiceTable,
    outcome: &OperationRequestOutcome,
) -> Option<PendingControlOperation> {
    if outcome.returned_operation_id != outcome.stored_operation_id {
        retain_live_boundaries(queue, operations);
        return None;
    }
    queue_control_boundary_for(queue, operations, services, outcome.returned_operation_id)
}

/// Hand the Pending operation `operation_id` to the control boundary, if it is
/// one the boundary executes (stop, reload, restart of a running service).
pub(super) fn queue_control_boundary_for(
    queue: &mut VecDeque<PendingControlOperation>,
    operations: &OperationStore,
    services: &ServiceTable,
    operation_id: OperationId,
) -> Option<PendingControlOperation> {
    retain_live_boundaries(queue, operations);
    let record = operations.get(operation_id)?;
    if record.state != OperationState::Pending {
        return None;
    }
    // Every requirement below acts on a running main process. A service in
    // Backoff has none: its pending restart is executed by the backoff
    // deadline, from the operation store, and sending it to the boundary
    // produced a `MissingCurrentMainJob` that ended the runtime loop
    // (PEI-803).
    if services
        .runtime(&record.service)
        .is_some_and(|runtime| runtime.state == ServiceState::Backoff)
    {
        return None;
    }
    // §8.3 queues this operation behind one still running. It reaches the
    // boundary when its predecessor finishes, through
    // `Supervisor::promote_queued_operations`, admitted against the state the
    // service is in by then. Sent now, a restart queued behind a stop ran its
    // own stop leg against a service already Stopping and the InvalidTransition
    // ended the runtime loop (PEI-824).
    if operations.is_queued_behind_live(record.id) {
        return None;
    }
    let requirement = requirement_for(record.operation_type)?;
    let pending = PendingControlOperation {
        operation_id: record.id,
        service: record.service.clone(),
        operation_type: record.operation_type,
        requirement,
    };
    if queue
        .iter()
        .any(|pending| pending.operation_id == record.id)
    {
        return Some(pending);
    }
    queue.push_back(pending.clone());
    Some(pending)
}

fn retain_live_boundaries(
    queue: &mut VecDeque<PendingControlOperation>,
    operations: &OperationStore,
) {
    queue.retain(|pending| {
        operations
            .get(pending.operation_id)
            .is_some_and(|operation| !operation.state.is_terminal())
    });
}

fn requirement_for(operation_type: OperationType) -> Option<PendingControlRequirement> {
    match operation_type {
        OperationType::Stop => Some(PendingControlRequirement::StopProcess),
        OperationType::Reload => Some(PendingControlRequirement::ReloadProcess),
        OperationType::Restart => Some(PendingControlRequirement::RestartProcess),
        OperationType::Start | OperationType::Reset => None,
    }
}
