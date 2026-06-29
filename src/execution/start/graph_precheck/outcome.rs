use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceActivationSnapshot, ServiceCheck};

use super::super::checks::format_check;
use super::super::deadline::start_operation_deadline_ns;
use super::super::model::{
    GraphPreStartCheckOutcome, GraphPreStartCheckPassedDispatch, GraphPreStartCheckPendingDispatch,
    GraphPreStartCheckTerminalDispatch, StartExecutionError, StartExecutionRequest,
    StartPreCheckTerminalOutcome,
};
use super::super::pre_dependency::{
    apply_pending_pre_dependency_failure, cancel_dormant_dependencies,
};
use super::super::store::{PendingPreStartCheck, PendingPreStartCheckStart, PrecheckedGraphStart};
use super::GraphPreStartCheckTransaction;

pub(super) fn apply_passed(
    transaction: &mut GraphPreStartCheckTransaction,
    request: StartExecutionRequest,
    activation: ServiceActivationSnapshot,
) -> Result<GraphPreStartCheckOutcome, StartExecutionError> {
    transaction
        .start_store
        .record_prechecked_graph_start(PrecheckedGraphStart {
            ready: request.ready.clone(),
            activation,
            resolved_identity: request.resolved_identity,
            token_summary: request.token_summary,
            checked_at_ns: request.started_at_ns,
        });
    let graph_context_ids = transaction
        .graph
        .apply_pre_start_check_passed(request.ready.operation_id)
        .map_err(StartExecutionError::Graph)?;

    Ok(GraphPreStartCheckOutcome::Passed(
        GraphPreStartCheckPassedDispatch {
            service: request.ready.service.clone(),
            operation_id: request.ready.operation_id,
            ready: request.ready,
            graph_context_ids,
        },
    ))
}

pub(super) fn apply_condition_skipped(
    transaction: &mut GraphPreStartCheckTransaction,
    request: StartExecutionRequest,
    check: ServiceCheck,
) -> Result<GraphPreStartCheckOutcome, StartExecutionError> {
    let check = format_check(&check);
    let transition = transaction
        .services
        .transition_service(
            &request.ready.service,
            ServiceTransition {
                to: ServiceState::Skipped,
                cause: TransitionCause::ConditionSkipped,
            },
        )
        .map_err(StartExecutionError::ServiceTable)?;
    transaction
        .services
        .mark_dependent_satisfied_since(&request.ready.service, request.started_at_ns)
        .map_err(StartExecutionError::ServiceTable)?;
    let completed = transaction
        .operations
        .complete_operation(
            request.ready.operation_id,
            request.started_at_ns,
            format!("ConditionSkipped: {check} not satisfied"),
        )
        .map_err(StartExecutionError::OperationStore)?;
    let graph_events = transaction
        .graph
        .apply_operation_satisfied(request.ready.operation_id)
        .map_err(StartExecutionError::Graph)?;
    let mut operation_events = vec![completed];
    operation_events.extend(cancel_dormant_dependencies(
        &mut transaction.operations,
        &mut transaction.graph,
        request.ready.operation_id,
        request.started_at_ns,
    )?);

    Ok(GraphPreStartCheckOutcome::Terminal(
        GraphPreStartCheckTerminalDispatch {
            ready: request.ready,
            outcome: StartPreCheckTerminalOutcome::ConditionSkipped { check },
            operation_events,
            service_transitions: vec![transition],
            graph_events,
        },
    ))
}

pub(super) fn apply_assertion_failed(
    transaction: &mut GraphPreStartCheckTransaction,
    request: StartExecutionRequest,
    check: ServiceCheck,
) -> Result<GraphPreStartCheckOutcome, StartExecutionError> {
    let check = format_check(&check);
    let failure = apply_pending_pre_dependency_failure(
        &mut transaction.services,
        &mut transaction.operations,
        &mut transaction.graph,
        request.ready.operation_id,
        request.started_at_ns,
        TransitionCause::AssertionError,
        format!("AssertionError: {check} not satisfied"),
    )?;

    Ok(GraphPreStartCheckOutcome::Terminal(
        GraphPreStartCheckTerminalDispatch {
            ready: request.ready,
            outcome: StartPreCheckTerminalOutcome::AssertionFailed { check },
            operation_events: failure.operation_events,
            service_transitions: failure.service_transitions,
            graph_events: failure.graph_events,
        },
    ))
}

pub(super) fn apply_filesystem_pending(
    transaction: &mut GraphPreStartCheckTransaction,
    request: StartExecutionRequest,
    activation: ServiceActivationSnapshot,
    checks: Vec<ServiceCheck>,
) -> Result<GraphPreStartCheckOutcome, StartExecutionError> {
    let pending = PendingPreStartCheck::new(
        request.ready.operation_id,
        request.ready.service.clone(),
        activation.clone(),
        checks,
        request.started_at_ns,
        start_operation_deadline_ns(
            transaction
                .operations
                .get(request.ready.operation_id)
                .ok_or(
                    crate::operation::store::OperationStoreError::UnknownOperation {
                        id: request.ready.operation_id,
                    },
                )
                .map_err(StartExecutionError::OperationStore)?,
            &activation.definition,
            request.started_at_ns,
        ),
        PendingPreStartCheckStart::GraphPreDependency {
            ready: request.ready.clone(),
            resolved_identity: request.resolved_identity,
            token_summary: request.token_summary,
        },
    );
    let registration = transaction
        .start_store
        .record_pending_pre_start_check(pending);

    Ok(GraphPreStartCheckOutcome::CheckPending(
        GraphPreStartCheckPendingDispatch {
            ready: request.ready.clone(),
            service: request.ready.service,
            operation_id: request.ready.operation_id,
            checks: registration.checks,
            helper_cgroup_id: registration.helper_cgroup_id,
        },
    ))
}
