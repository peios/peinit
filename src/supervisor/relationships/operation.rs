use crate::control::lifecycle::LifecycleCommandError;
use crate::ids::OperationId;
use crate::operation::store::OperationRequest;
use crate::operation::{OperationSource, OperationType};
use crate::supervisor::control_boundary::queue_control_boundary;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

pub(super) fn queue_relationship_stop(
    work: &mut SupervisorWork,
    service: &str,
    source: OperationSource,
    observed_at_ns: u64,
) -> Result<(), SupervisorError> {
    let operation_id = allocate_operation_id(work, observed_at_ns)?;
    let outcome = work
        .operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Stop,
            service: service.to_string(),
            source,
            caller: None,
            created_at_ns: observed_at_ns,
        })
        .map_err(|source| {
            SupervisorError::Lifecycle(LifecycleCommandError::OperationStore(source))
        })?;
    queue_control_boundary(
        &mut work.pending_control_operations,
        &work.operations,
        &work.services,
        &outcome,
    );
    Ok(())
}

pub(super) fn allocate_operation_id(
    work: &mut SupervisorWork,
    observed_at_ns: u64,
) -> Result<OperationId, SupervisorError> {
    Ok(work
        .operation_ids
        .allocate_batch(1, observed_at_ns)
        .map_err(SupervisorError::RequestIdAllocation)?[0])
}
