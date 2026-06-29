use crate::control::lifecycle::{
    LifecycleCommandOutcome, LifecycleCommandRequest, admit_lifecycle_command_with_operation_ids,
};

use super::super::state::SupervisorError;
use super::super::work::SupervisorWork;

pub(in crate::supervisor::lifecycle) fn allocate_request_id(
    work: &mut SupervisorWork,
    observed_at_ns: u64,
) -> Result<crate::ids::OperationId, SupervisorError> {
    Ok(work
        .operation_ids
        .allocate_batch(1, observed_at_ns)
        .map_err(SupervisorError::RequestIdAllocation)?[0])
}

pub(in crate::supervisor::lifecycle) fn admit_supervisor_lifecycle_command(
    work: &mut SupervisorWork,
    request: LifecycleCommandRequest,
) -> Result<LifecycleCommandOutcome, SupervisorError> {
    admit_lifecycle_command_with_operation_ids(
        &mut work.services,
        &mut work.operations,
        &mut work.operation_ids,
        request,
    )
    .map_err(SupervisorError::Lifecycle)
}
