use crate::execution::graph::{GraphExecutionEvent, GraphExecutionStore};
use crate::execution::start_validation::validate_running_start_operation;
use crate::ids::OperationId;
use crate::operation::store::OperationStore;
use crate::service::runtime::{ServiceTransition, TransitionCause};
use crate::service::{
    RestartEvaluationAction, ServiceTable, ServiceTableError, evaluate_restart_after_failure,
};

use super::model::{StartFailureDispatch, StartFailureError, StartFailureRequest};

const DEPENDENCY_FAILURE_REASON: &str = "DependencyFailure: dependency failed during graph start";
const NANOS_PER_SEC: u64 = 1_000_000_000;

pub fn apply_start_failure(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    request: StartFailureRequest,
) -> Result<StartFailureDispatch, StartFailureError> {
    validate_running_start_operation(operations, &request.service, request.operation_id)
        .map_err(StartFailureError::from)?;

    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_graph = graph.clone();

    let graph_events = next_graph
        .apply_operation_failed(request.operation_id)
        .map_err(StartFailureError::Graph)?;
    let terminal_operations = terminal_operations(&request, &graph_events);
    let mut service_transitions = Vec::with_capacity(terminal_operations.len());
    let mut operation_events = Vec::with_capacity(terminal_operations.len());

    for terminal in terminal_operations {
        let transition =
            transition_after_start_failure(&mut next_services, &terminal, request.failed_at_ns)
                .map_err(StartFailureError::ServiceTable)?;
        let operation_event = next_operations
            .fail_operation(terminal.operation_id, request.failed_at_ns, terminal.reason)
            .map_err(StartFailureError::OperationStore)?;
        service_transitions.push(transition);
        operation_events.push(operation_event);
    }

    *services = next_services;
    *operations = next_operations;
    *graph = next_graph;

    Ok(StartFailureDispatch {
        operation_events,
        service_transitions,
        graph_events,
    })
}

fn terminal_operations(
    request: &StartFailureRequest,
    graph_events: &[GraphExecutionEvent],
) -> Vec<TerminalOperation> {
    let mut terminal = Vec::new();
    for event in graph_events {
        push_unique_terminal(&mut terminal, terminal_from_graph_event(request, event));
    }

    if terminal.is_empty() {
        terminal.push(TerminalOperation {
            service: request.service.clone(),
            operation_id: request.operation_id,
            cause: request.failure_cause,
            reason: request.reason.clone(),
        });
    }

    terminal
}

fn terminal_from_graph_event(
    request: &StartFailureRequest,
    event: &GraphExecutionEvent,
) -> TerminalOperation {
    if event.operation_id == request.operation_id {
        return TerminalOperation {
            service: event.service.clone(),
            operation_id: event.operation_id,
            cause: request.failure_cause,
            reason: request.reason.clone(),
        };
    }

    TerminalOperation {
        service: event.service.clone(),
        operation_id: event.operation_id,
        cause: TransitionCause::DependencyFailure,
        reason: DEPENDENCY_FAILURE_REASON.to_string(),
    }
}

fn push_unique_terminal(terminal: &mut Vec<TerminalOperation>, candidate: TerminalOperation) {
    if terminal
        .iter()
        .all(|existing| existing.operation_id != candidate.operation_id)
    {
        terminal.push(candidate);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalOperation {
    service: String,
    operation_id: OperationId,
    cause: TransitionCause,
    reason: String,
}

fn transition_after_start_failure(
    services: &mut ServiceTable,
    terminal: &TerminalOperation,
    failed_at_ns: u64,
) -> Result<crate::service::ServiceTableTransition, ServiceTableError> {
    let definition = services.definition(&terminal.service).ok_or_else(|| {
        ServiceTableError::UnknownService {
            service: terminal.service.clone(),
        }
    })?;
    let consecutive_failures = services
        .runtime(&terminal.service)
        .ok_or_else(|| ServiceTableError::UnknownService {
            service: terminal.service.clone(),
        })?
        .consecutive_restart_failures;
    let evaluation =
        evaluate_restart_after_failure(definition, terminal.cause, None, consecutive_failures);

    match evaluation.action {
        RestartEvaluationAction::Backoff {
            cause, delay_secs, ..
        } => services.transition_service_to_restart_backoff(
            &terminal.service,
            cause,
            failed_at_ns.saturating_add(delay_secs.saturating_mul(NANOS_PER_SEC)),
        ),
        RestartEvaluationAction::Fail { cause, state } => {
            services.transition_service(&terminal.service, ServiceTransition { to: state, cause })
        }
    }
}
