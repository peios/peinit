use crate::execution::graph::{GraphExecutionEvent, GraphExecutionStore};
use crate::execution::start_validation::validate_running_start_operation;
use crate::operation::store::{OperationEvent, OperationStore};
use crate::service::ServiceTable;

use super::super::model::StartExecutionError;
use super::super::store::PostStartHookSequence;
use super::job::post_start_result;

pub(super) struct PostStartCompletion {
    pub(super) operation_events: Vec<OperationEvent>,
    pub(super) service_transitions: Vec<crate::service::ServiceTableTransition>,
    pub(super) graph_events: Vec<GraphExecutionEvent>,
}

pub(super) fn complete_post_start_operation(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    sequence: &PostStartHookSequence,
    completed_at_ns: u64,
) -> Result<PostStartCompletion, StartExecutionError> {
    validate_running_start_operation(operations, &sequence.service, sequence.operation_id)
        .map_err(|error| StartExecutionError::StartSatisfaction(error.into()))?;
    services
        .mark_dependent_satisfied_since(&sequence.service, completed_at_ns)
        .map_err(StartExecutionError::ServiceTable)?;
    let operation_event = operations
        .complete_operation(
            sequence.operation_id,
            completed_at_ns,
            post_start_result(sequence),
        )
        .map_err(StartExecutionError::OperationStore)?;
    let graph_events = graph
        .apply_operation_satisfied(sequence.operation_id)
        .map_err(StartExecutionError::Graph)?;
    let service_transitions = services
        .release_completed_oneshot_if_not_retained(&sequence.service)
        .map_err(StartExecutionError::ServiceTable)?
        .into_iter()
        .collect();

    Ok(PostStartCompletion {
        operation_events: vec![operation_event],
        service_transitions,
        graph_events,
    })
}
