use std::collections::BTreeSet;

use crate::execution::graph::{GraphExecutionEvent, GraphExecutionStore, GraphTerminalOutcome};
use crate::execution::start_validation::validate_running_start_operation;
use crate::operation::store::{OperationEvent, OperationStore};
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{
    RestartEvaluation, RestartEvaluationAction, ServiceTable, ServiceTableError,
    ServiceTableTransition, evaluate_restart_after_failure,
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

    // The restart evaluation comes first, because it decides what the
    // failure *is* to everything waiting on this service. A service that is
    // going to start again has not failed its dependents: they wait for the
    // restart, exactly as they would wait for a readiness level (§6.1).
    // Failing the graph member before consulting the policy gave every hard
    // dependent a DependencyFailure on the way to a Backoff the target then
    // came back from (PEI-821).
    let evaluation = restart_evaluation(&next_services, &request)?;
    let dispatch = match evaluation.action {
        RestartEvaluationAction::Backoff {
            cause, delay_secs, ..
        } => hold_for_restart(
            &mut next_services,
            &mut next_operations,
            &mut next_graph,
            &request,
            cause,
            delay_secs,
        )?,
        RestartEvaluationAction::Fail { cause, state } => fail_with_dependents(
            &mut next_services,
            &mut next_operations,
            &mut next_graph,
            &request,
            cause,
            state,
        )?,
    };

    *services = next_services;
    *operations = next_operations;
    *graph = next_graph;

    Ok(dispatch)
}

/// The dependents a decided hold failed alongside their target.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DependentFailures {
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
}

/// Fail the hard dependents named by `graph_events` as dependency failures.
///
/// The target's own events are skipped — the caller has already decided
/// the target — and so is any dependent whose operation is already
/// terminal, because the same dependent can be named twice when a service
/// is held in more than one context. Each dependent is failed at most once.
pub fn fail_dependents_after_graph_events(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph_events: &[GraphExecutionEvent],
    failed_target: &str,
    failed_at_ns: u64,
    reason: &str,
) -> Result<DependentFailures, StartFailureError> {
    let mut failures = DependentFailures::default();
    let mut seen = BTreeSet::new();
    for event in graph_events {
        if event.outcome != GraphTerminalOutcome::Failed
            || event.service == failed_target
            || !seen.insert(event.service.clone())
        {
            continue;
        }
        if operations
            .get(event.operation_id)
            .is_none_or(|operation| operation.state.is_terminal())
        {
            continue;
        }
        // A held dependent is Inactive. One something else has already moved
        // — a reload that failed its validation, say — keeps that state; its
        // pending start still ends here, because the dependency it waited
        // for is gone either way.
        if services
            .runtime(&event.service)
            .is_some_and(|runtime| runtime.state == ServiceState::Inactive)
        {
            // A dependent cancelled by its target's failure did not exit at
            // all, so no exit code is passed on: one service's success code
            // must not excuse another's dependency failure.
            let transition = transition_after_failure(
                services,
                &event.service,
                TransitionCause::DependencyFailure,
                failed_at_ns,
                None,
            )
            .map_err(StartFailureError::ServiceTable)?;
            failures.service_transitions.push(transition);
        }
        let operation_event = operations
            .fail_operation(event.operation_id, failed_at_ns, reason)
            .map_err(StartFailureError::OperationStore)?;
        failures.operation_events.push(operation_event);
    }
    Ok(failures)
}

fn hold_for_restart(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    request: &StartFailureRequest,
    cause: TransitionCause,
    delay_secs: u64,
) -> Result<StartFailureDispatch, StartFailureError> {
    graph
        .apply_operation_awaiting_restart(request.operation_id)
        .map_err(StartFailureError::Graph)?;
    let transition = services
        .transition_service_to_restart_backoff(
            &request.service,
            cause,
            request
                .failed_at_ns
                .saturating_add(delay_secs.saturating_mul(NANOS_PER_SEC)),
        )
        .map_err(StartFailureError::ServiceTable)?;
    let operation_event = operations
        .fail_operation(
            request.operation_id,
            request.failed_at_ns,
            request.reason.clone(),
        )
        .map_err(StartFailureError::OperationStore)?;
    Ok(StartFailureDispatch {
        operation_events: vec![operation_event],
        service_transitions: vec![transition],
        // The member is held, not terminal: there is nothing to release and
        // nothing for a dependent to react to yet.
        graph_events: Vec::new(),
    })
}

fn fail_with_dependents(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    request: &StartFailureRequest,
    cause: TransitionCause,
    state: ServiceState,
) -> Result<StartFailureDispatch, StartFailureError> {
    // A relaunch that fails for good also decides every hold on this service
    // in the contexts that were waiting for it to come back: the graph
    // settles those alongside the operation's own context, and their
    // dependents are in `graph_events` like any other.
    let graph_events = graph
        .apply_operation_failed(request.operation_id)
        .map_err(StartFailureError::Graph)?;
    let transition = services
        .transition_service(&request.service, ServiceTransition { to: state, cause })
        .map_err(StartFailureError::ServiceTable)?;
    let operation_event = operations
        .fail_operation(
            request.operation_id,
            request.failed_at_ns,
            request.reason.clone(),
        )
        .map_err(StartFailureError::OperationStore)?;
    let dependents = fail_dependents_after_graph_events(
        services,
        operations,
        &graph_events,
        &request.service,
        request.failed_at_ns,
        DEPENDENCY_FAILURE_REASON,
    )?;

    let mut operation_events = vec![operation_event];
    operation_events.extend(dependents.operation_events);
    let mut service_transitions = vec![transition];
    service_transitions.extend(dependents.service_transitions);
    Ok(StartFailureDispatch {
        operation_events,
        service_transitions,
        graph_events,
    })
}

fn restart_evaluation(
    services: &ServiceTable,
    request: &StartFailureRequest,
) -> Result<RestartEvaluation, StartFailureError> {
    let definition = services.definition(&request.service).ok_or_else(|| {
        StartFailureError::ServiceTable(ServiceTableError::UnknownService {
            service: request.service.clone(),
        })
    })?;
    let consecutive_failures = services
        .runtime(&request.service)
        .ok_or_else(|| {
            StartFailureError::ServiceTable(ServiceTableError::UnknownService {
                service: request.service.clone(),
            })
        })?
        .consecutive_restart_failures;
    Ok(evaluate_restart_after_failure(
        definition,
        request.failure_cause,
        request.exit_code,
        consecutive_failures,
    ))
}

fn transition_after_failure(
    services: &mut ServiceTable,
    service: &str,
    cause: TransitionCause,
    failed_at_ns: u64,
    exit_code: Option<i32>,
) -> Result<ServiceTableTransition, ServiceTableError> {
    let definition =
        services
            .definition(service)
            .ok_or_else(|| ServiceTableError::UnknownService {
                service: service.to_string(),
            })?;
    let consecutive_failures = services
        .runtime(service)
        .ok_or_else(|| ServiceTableError::UnknownService {
            service: service.to_string(),
        })?
        .consecutive_restart_failures;
    let evaluation =
        evaluate_restart_after_failure(definition, cause, exit_code, consecutive_failures);

    match evaluation.action {
        RestartEvaluationAction::Backoff {
            cause, delay_secs, ..
        } => services.transition_service_to_restart_backoff(
            service,
            cause,
            failed_at_ns.saturating_add(delay_secs.saturating_mul(NANOS_PER_SEC)),
        ),
        RestartEvaluationAction::Fail { cause, state } => {
            services.transition_service(service, ServiceTransition { to: state, cause })
        }
    }
}
