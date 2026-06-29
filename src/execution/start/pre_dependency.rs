use crate::execution::graph::{GraphExecutionEvent, GraphExecutionStore, GraphPrunedOperation};
use crate::ids::OperationId;
use crate::operation::store::{OperationEvent, OperationStore};
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceTable, ServiceTableTransition};

use super::model::StartExecutionError;

const DEPENDENCY_FAILURE_REASON: &str = "DependencyFailure: dependency failed during graph start";
const PRECHECK_PRUNED_REASON: &str =
    "DependencyPruned: dependency was not resolved because dependent precheck did not pass";

pub(super) struct PendingPreDependencyFailure {
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
}

pub(super) fn apply_pending_pre_dependency_failure(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    operation_id: OperationId,
    failed_at_ns: u64,
    failure_cause: TransitionCause,
    reason: String,
) -> Result<PendingPreDependencyFailure, StartExecutionError> {
    let graph_events = graph
        .apply_operation_failed(operation_id)
        .map_err(StartExecutionError::Graph)?;
    let mut operation_events = Vec::new();
    let mut service_transitions = Vec::new();

    for event in &graph_events {
        let (cause, result) = if event.operation_id == operation_id {
            (failure_cause, reason.clone())
        } else {
            (
                TransitionCause::DependencyFailure,
                DEPENDENCY_FAILURE_REASON.to_string(),
            )
        };
        service_transitions.push(
            services
                .transition_service(
                    &event.service,
                    ServiceTransition {
                        to: ServiceState::Failed,
                        cause,
                    },
                )
                .map_err(StartExecutionError::ServiceTable)?,
        );
        operation_events.push(
            operations
                .fail_operation(event.operation_id, failed_at_ns, result)
                .map_err(StartExecutionError::OperationStore)?,
        );
    }
    operation_events.extend(cancel_dormant_dependencies(
        operations,
        graph,
        operation_id,
        failed_at_ns,
    )?);

    Ok(PendingPreDependencyFailure {
        operation_events,
        service_transitions,
        graph_events,
    })
}

pub(super) fn cancel_dormant_dependencies(
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    operation_id: OperationId,
    observed_at_ns: u64,
) -> Result<Vec<OperationEvent>, StartExecutionError> {
    let pruned = graph
        .prune_dormant_dependencies(operation_id)
        .map_err(StartExecutionError::Graph)?;
    cancel_pruned_operations(operations, &pruned, observed_at_ns)
}

fn cancel_pruned_operations(
    operations: &mut OperationStore,
    pruned: &[GraphPrunedOperation],
    observed_at_ns: u64,
) -> Result<Vec<OperationEvent>, StartExecutionError> {
    pruned
        .iter()
        .map(|pruned| {
            operations
                .cancel_operation(pruned.operation_id, observed_at_ns, PRECHECK_PRUNED_REASON)
                .map_err(StartExecutionError::OperationStore)
        })
        .collect()
}
