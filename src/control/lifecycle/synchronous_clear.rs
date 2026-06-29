use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::admission::operation_request;
use super::model::{
    LifecycleCommandError, LifecycleCommandOutcome, LifecycleCommandRequest,
    SynchronousClearOutcome,
};

pub(super) fn admit_synchronous_clear(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    request: LifecycleCommandRequest,
    cause: TransitionCause,
    result: &'static str,
) -> Result<LifecycleCommandOutcome, LifecycleCommandError> {
    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let operation = operation_request(request);
    let id = operation.id;
    let service = operation.service.clone();
    let created_at_ns = operation.created_at_ns;
    let request = next_operations
        .request_operation(operation)
        .map_err(LifecycleCommandError::OperationStore)?;
    let started = next_operations
        .start_operation(id, created_at_ns)
        .map_err(LifecycleCommandError::OperationStore)?;
    let service_transition = next_services
        .transition_service(
            &service,
            ServiceTransition {
                to: ServiceState::Inactive,
                cause,
            },
        )
        .map_err(LifecycleCommandError::ServiceTable)?;
    let completed = next_operations
        .complete_operation(id, created_at_ns, result)
        .map_err(LifecycleCommandError::OperationStore)?;

    *services = next_services;
    *operations = next_operations;

    Ok(LifecycleCommandOutcome::SynchronousClear(Box::new(
        SynchronousClearOutcome {
            request,
            started,
            service_transition,
            completed,
        },
    )))
}
