use crate::service::runtime::{ServiceState, ServiceTransition};

use super::super::initial::{InitialStartJobRequest, create_initial_start_job};
use super::super::job_id::job_id_for_ready_start;
use super::super::model::{PreStartCheckCompletionDispatch, StartExecutionError};
use super::super::store::{PendingPreStartCheck, PendingPreStartCheckStart, PrecheckedGraphStart};
use super::transaction::PreStartCheckTransaction;

pub(super) fn apply_check_passed(
    transaction: &mut PreStartCheckTransaction,
    result_fd: i32,
    pending: PendingPreStartCheck,
) -> Result<PreStartCheckCompletionDispatch, StartExecutionError> {
    // Read before `pending.start` is moved by the match below. This is the
    // `Skipped -> Inactive` the start performed before the helper ran; it is
    // reported here, with the transition the checks' outcome produces.
    let cleared_skipped = pending.cleared_skipped.clone();
    let (
        job_id,
        operation_id,
        resolved_identity,
        token_summary,
        service_transition,
        graph_context_ids,
    ) = match pending.start {
        PendingPreStartCheckStart::Graph {
            ready,
            resolved_identity,
            token_summary,
        } => {
            let transition = transaction
                .services
                .transition_service(
                    &ready.service,
                    ServiceTransition {
                        to: ServiceState::Starting,
                        cause: ready.transition_cause,
                    },
                )
                .map_err(StartExecutionError::ServiceTable)?;
            (
                job_id_for_ready_start(
                    &mut transaction.job_ids,
                    ready.reserved_job_id,
                    pending.started_at_ns,
                )?,
                ready.operation_id,
                resolved_identity,
                token_summary,
                Some(transition),
                Vec::new(),
            )
        }
        PendingPreStartCheckStart::GraphPreDependency {
            ready,
            resolved_identity,
            token_summary,
        } => {
            transaction
                .start_store
                .record_prechecked_graph_start(PrecheckedGraphStart {
                    ready,
                    activation: pending.activation,
                    resolved_identity,
                    token_summary,
                    checked_at_ns: pending.started_at_ns,
                    cleared_skipped,
                });
            let graph_context_ids = transaction
                .graph
                .apply_pre_start_check_passed(pending.operation_id)
                .map_err(StartExecutionError::Graph)?;
            return Ok(PreStartCheckCompletionDispatch {
                result_fd,
                job_id: None,
                job_event: None,
                job_kind: None,
                operation_events: Vec::new(),
                service_transitions: Vec::new(),
                graph_events: Vec::new(),
                graph_context_ids,
            });
        }
        PendingPreStartCheckStart::Restart {
            resolved_identity,
            token_summary,
        } => (
            transaction
                .job_ids
                .allocate_batch(1, pending.started_at_ns)
                .map(|ids| ids[0])
                .map_err(StartExecutionError::JobIdAllocation)?,
            pending.operation_id,
            resolved_identity,
            token_summary,
            None,
            Vec::new(),
        ),
    };

    let initial = create_initial_start_job(
        &mut transaction.jobs,
        &mut transaction.job_ids,
        &mut transaction.start_store,
        InitialStartJobRequest {
            service: &pending.service,
            operation_id,
            resolved_identity,
            token_summary,
            started_at_ns: pending.started_at_ns,
            operation_deadline_ns: pending.operation_deadline_ns,
            activation: &pending.activation,
            main_job_id: job_id,
        },
    )?;

    Ok(PreStartCheckCompletionDispatch {
        result_fd,
        job_id: Some(initial.job_id),
        job_event: Some(initial.job_event),
        job_kind: Some(initial.job_kind),
        operation_events: Vec::new(),
        service_transitions: cleared_skipped
            .into_iter()
            .chain(service_transition)
            .collect(),
        graph_events: Vec::new(),
        graph_context_ids,
    })
}
