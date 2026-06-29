use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::super::failure::{StartFailureRequest, apply_start_failure};
use super::super::graph::GraphExecutionStore;
use super::checks::{PreStartCheckDecision, evaluate_cacheable_pre_start_checks, format_check};
use super::deadline::start_operation_deadline_ns;
use super::initial::{InitialStartJobRequest, create_initial_start_job};
use super::job_id::job_id_for_request;
use super::model::{
    StartExecutionCheckPendingDispatch, StartExecutionDispatch, StartExecutionError,
    StartExecutionOutcome, StartExecutionRequest, StartExecutionTerminalDispatch,
    StartPreCheckTerminalOutcome,
};
use super::store::{PendingPreStartCheck, PendingPreStartCheckStart, StartExecutionStore};

pub fn begin_ready_start(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    jobs: &mut JobStore,
    job_ids: &mut JobIdAllocator,
    start_store: &mut StartExecutionStore,
    request: StartExecutionRequest,
) -> Result<StartExecutionOutcome, StartExecutionError> {
    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_graph = graph.clone();
    let mut next_jobs = jobs.clone();
    let mut next_job_ids = job_ids.clone();
    let mut next_start_store = start_store.clone();

    let activation = next_services
        .prepare_activation_snapshot(&request.ready.service)
        .map_err(StartExecutionError::ServiceTable)?;
    let operation_event = next_operations
        .start_operation(request.ready.operation_id, request.started_at_ns)
        .map_err(StartExecutionError::OperationStore)?;
    let operation_deadline_ns = start_operation_deadline_ns(
        next_operations
            .get(request.ready.operation_id)
            .ok_or(
                crate::operation::store::OperationStoreError::UnknownOperation {
                    id: request.ready.operation_id,
                },
            )
            .map_err(StartExecutionError::OperationStore)?,
        &activation.definition,
        request.started_at_ns,
    );

    match evaluate_cacheable_pre_start_checks(
        &next_services,
        &activation.definition.conditions,
        &activation.definition.asserts,
    ) {
        PreStartCheckDecision::Passed => {}
        PreStartCheckDecision::ConditionSkipped(check) => {
            let transition = next_services
                .transition_service(
                    &request.ready.service,
                    ServiceTransition {
                        to: ServiceState::Skipped,
                        cause: TransitionCause::ConditionSkipped,
                    },
                )
                .map_err(StartExecutionError::ServiceTable)?;
            next_services
                .mark_dependent_satisfied_since(&request.ready.service, request.started_at_ns)
                .map_err(StartExecutionError::ServiceTable)?;
            let completed = next_operations
                .complete_operation(
                    request.ready.operation_id,
                    request.started_at_ns,
                    format!("ConditionSkipped: {} not satisfied", format_check(&check)),
                )
                .map_err(StartExecutionError::OperationStore)?;
            let graph_events = next_graph
                .apply_operation_satisfied(request.ready.operation_id)
                .map_err(StartExecutionError::Graph)?;

            *services = next_services;
            *operations = next_operations;
            *graph = next_graph;

            return Ok(StartExecutionOutcome::Terminal(
                StartExecutionTerminalDispatch {
                    ready: request.ready,
                    outcome: StartPreCheckTerminalOutcome::ConditionSkipped {
                        check: format_check(&check),
                    },
                    operation_events: vec![operation_event, completed],
                    service_transitions: vec![transition],
                    graph_events,
                },
            ));
        }
        PreStartCheckDecision::AssertionFailed(check) => {
            let failure = apply_start_failure(
                &mut next_services,
                &mut next_operations,
                &mut next_graph,
                StartFailureRequest {
                    service: request.ready.service.clone(),
                    operation_id: request.ready.operation_id,
                    failed_at_ns: request.started_at_ns,
                    failure_cause: TransitionCause::AssertionError,
                    reason: format!("AssertionError: {} not satisfied", format_check(&check)),
                },
            )
            .map_err(StartExecutionError::StartFailure)?;

            *services = next_services;
            *operations = next_operations;
            *graph = next_graph;

            return Ok(StartExecutionOutcome::Terminal(
                StartExecutionTerminalDispatch {
                    ready: request.ready,
                    outcome: StartPreCheckTerminalOutcome::AssertionFailed {
                        check: format_check(&check),
                    },
                    operation_events: std::iter::once(operation_event)
                        .chain(failure.operation_events)
                        .collect(),
                    service_transitions: failure.service_transitions,
                    graph_events: failure.graph_events,
                },
            ));
        }
        PreStartCheckDecision::RequiresFilesystemHelper { checks } => {
            let pending = PendingPreStartCheck::new(
                request.ready.operation_id,
                request.ready.service.clone(),
                activation,
                checks,
                request.started_at_ns,
                operation_deadline_ns,
                PendingPreStartCheckStart::Graph {
                    ready: request.ready.clone(),
                    resolved_identity: request.resolved_identity,
                    token_summary: request.token_summary,
                },
            );
            let registration = next_start_store.record_pending_pre_start_check(pending);

            *operations = next_operations;
            *start_store = next_start_store;

            return Ok(StartExecutionOutcome::CheckPending(
                StartExecutionCheckPendingDispatch {
                    ready: request.ready.clone(),
                    operation_event,
                    service: request.ready.service,
                    operation_id: request.ready.operation_id,
                    checks: registration.checks,
                    helper_cgroup_id: registration.helper_cgroup_id,
                },
            ));
        }
    }

    let main_job_id = job_id_for_request(&mut next_job_ids, &request)?;
    let service_transition = next_services
        .transition_service(
            &request.ready.service,
            ServiceTransition {
                to: ServiceState::Starting,
                cause: request.ready.transition_cause,
            },
        )
        .map_err(StartExecutionError::ServiceTable)?;
    let initial = create_initial_start_job(
        &mut next_jobs,
        &mut next_job_ids,
        &mut next_start_store,
        InitialStartJobRequest {
            service: &request.ready.service,
            operation_id: request.ready.operation_id,
            resolved_identity: request.resolved_identity.clone(),
            token_summary: request.token_summary.clone(),
            started_at_ns: request.started_at_ns,
            operation_deadline_ns,
            activation: &activation,
            main_job_id,
        },
    )?;

    *services = next_services;
    *operations = next_operations;
    *graph = next_graph;
    *jobs = next_jobs;
    *job_ids = next_job_ids;
    *start_store = next_start_store;

    Ok(StartExecutionOutcome::Job(Box::new(
        StartExecutionDispatch {
            ready: request.ready,
            job_id: initial.job_id,
            operation_event,
            service_transition,
            job_event: initial.job_event,
            job_kind: initial.job_kind,
        },
    )))
}
