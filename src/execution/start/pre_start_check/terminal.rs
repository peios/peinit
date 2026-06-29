use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::super::model::{PreStartCheckCompletionDispatch, StartExecutionError};
use super::super::pre_dependency::{
    apply_pending_pre_dependency_failure, cancel_dormant_dependencies,
};
use super::super::store::{PendingPreStartCheck, PendingPreStartCheckStart};
use super::transaction::PreStartCheckTransaction;

pub(super) fn apply_condition_skipped(
    transaction: &mut PreStartCheckTransaction,
    result_fd: i32,
    pending: PendingPreStartCheck,
    reason: String,
) -> Result<PreStartCheckCompletionDispatch, StartExecutionError> {
    let transition = transaction
        .services
        .transition_service(
            &pending.service,
            ServiceTransition {
                to: ServiceState::Skipped,
                cause: TransitionCause::ConditionSkipped,
            },
        )
        .map_err(StartExecutionError::ServiceTable)?;
    transaction
        .services
        .mark_dependent_satisfied_since(&pending.service, pending.started_at_ns)
        .map_err(StartExecutionError::ServiceTable)?;
    let operation_event = transaction
        .operations
        .complete_operation(pending.operation_id, pending.started_at_ns, reason)
        .map_err(StartExecutionError::OperationStore)?;
    let is_graph_start = matches!(
        pending.start,
        PendingPreStartCheckStart::Graph { .. }
            | PendingPreStartCheckStart::GraphPreDependency { .. }
    );
    let graph_events = if is_graph_start {
        transaction
            .graph
            .apply_operation_satisfied(pending.operation_id)
            .map_err(StartExecutionError::Graph)?
    } else {
        Vec::new()
    };
    let pruned_operation_events = if is_graph_start {
        cancel_dormant_dependencies(
            &mut transaction.operations,
            &mut transaction.graph,
            pending.operation_id,
            pending.started_at_ns,
        )?
    } else {
        Vec::new()
    };
    let mut operation_events = vec![operation_event];
    operation_events.extend(pruned_operation_events);

    Ok(PreStartCheckCompletionDispatch {
        result_fd,
        job_id: None,
        job_event: None,
        job_kind: None,
        operation_events,
        service_transitions: vec![transition],
        graph_events,
        graph_context_ids: Vec::new(),
    })
}

pub(super) fn apply_assertion_failed(
    transaction: &mut PreStartCheckTransaction,
    result_fd: i32,
    pending: PendingPreStartCheck,
    reason: String,
) -> Result<PreStartCheckCompletionDispatch, StartExecutionError> {
    if matches!(
        pending.start,
        PendingPreStartCheckStart::GraphPreDependency { .. }
    ) {
        let failure = apply_pending_pre_dependency_failure(
            &mut transaction.services,
            &mut transaction.operations,
            &mut transaction.graph,
            pending.operation_id,
            pending.started_at_ns,
            TransitionCause::AssertionError,
            reason,
        )?;
        return Ok(PreStartCheckCompletionDispatch {
            result_fd,
            job_id: None,
            job_event: None,
            job_kind: None,
            operation_events: failure.operation_events,
            service_transitions: failure.service_transitions,
            graph_events: failure.graph_events,
            graph_context_ids: Vec::new(),
        });
    }

    let failure = apply_start_failure(
        &mut transaction.services,
        &mut transaction.operations,
        &mut transaction.graph,
        StartFailureRequest {
            service: pending.service,
            operation_id: pending.operation_id,
            failed_at_ns: pending.started_at_ns,
            failure_cause: TransitionCause::AssertionError,
            reason,
        },
    )
    .map_err(StartExecutionError::StartFailure)?;

    Ok(PreStartCheckCompletionDispatch {
        result_fd,
        job_id: None,
        job_event: None,
        job_kind: None,
        operation_events: failure.operation_events,
        service_transitions: failure.service_transitions,
        graph_events: failure.graph_events,
        graph_context_ids: Vec::new(),
    })
}
