use crate::execution::graph::GraphExecutionStore;
use crate::execution::start_validation::{
    start_transition_cause, validate_running_start_operation,
};
use crate::operation::store::OperationStore;
use crate::service::runtime::{ServiceState, ServiceTransition};
use crate::service::{ServiceTable, ServiceTableError, ServiceType};

use super::model::{StartSatisfactionDispatch, StartSatisfactionError, StartSatisfactionRequest};

pub fn apply_start_satisfaction(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    request: StartSatisfactionRequest,
) -> Result<StartSatisfactionDispatch, StartSatisfactionError> {
    validate_running_start_operation(operations, &request.service, request.operation_id)
        .map_err(StartSatisfactionError::from)?;

    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_graph = graph.clone();

    let transition = next_services
        .transition_service(
            &request.service,
            ServiceTransition {
                to: satisfied_state(&next_services, &request.service)?,
                cause: start_transition_cause(&next_operations, request.operation_id)
                    .map_err(StartSatisfactionError::from)?,
            },
        )
        .map_err(StartSatisfactionError::ServiceTable)?;
    next_services
        .mark_dependent_satisfied_since(&request.service, request.satisfied_at_ns)
        .map_err(StartSatisfactionError::ServiceTable)?;
    let operation_event = next_operations
        .complete_operation(
            request.operation_id,
            request.satisfied_at_ns,
            request.result,
        )
        .map_err(StartSatisfactionError::OperationStore)?;
    let graph_events = next_graph
        .apply_operation_satisfied(request.operation_id)
        .map_err(StartSatisfactionError::Graph)?;
    let mut service_transitions = vec![transition];
    if let Some(release) = next_services
        .release_completed_oneshot_if_not_retained(&request.service)
        .map_err(StartSatisfactionError::ServiceTable)?
    {
        service_transitions.push(release);
    }

    *services = next_services;
    *operations = next_operations;
    *graph = next_graph;

    Ok(StartSatisfactionDispatch {
        operation_event,
        service_transitions,
        graph_events,
    })
}

fn satisfied_state(
    services: &ServiceTable,
    service: &str,
) -> Result<ServiceState, StartSatisfactionError> {
    let definition = services.definition(service).ok_or_else(|| {
        StartSatisfactionError::ServiceTable(ServiceTableError::UnknownService {
            service: service.to_string(),
        })
    })?;
    Ok(match definition.service_type {
        ServiceType::Simple => ServiceState::Active,
        ServiceType::Oneshot => ServiceState::Completed,
    })
}
