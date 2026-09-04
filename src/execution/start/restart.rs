use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::store::{OperationStore, OperationStoreError};
use crate::operation::{
    OperationState, OperationTransitionAction, OperationTransitionError, OperationType,
};
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use crate::execution::failure::{StartFailureRequest, apply_start_failure};

use super::checks::{PreStartCheckDecision, evaluate_cacheable_pre_start_checks, format_check};
use super::deadline::start_operation_deadline_ns;
use super::initial::{InitialStartJobRequest, create_initial_start_job};
use super::model::{
    RestartStartExecutionCheckPendingDispatch, RestartStartExecutionDispatch,
    RestartStartExecutionOutcome, RestartStartExecutionRequest,
    RestartStartExecutionTerminalDispatch, StartExecutionError, StartPreCheckTerminalOutcome,
};
use super::store::{PendingPreStartCheck, PendingPreStartCheckStart, StartExecutionStore};

pub fn begin_restart_start_leg(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut crate::execution::graph::GraphExecutionStore,
    jobs: &mut JobStore,
    job_ids: &mut JobIdAllocator,
    start_store: &mut StartExecutionStore,
    request: RestartStartExecutionRequest,
) -> Result<RestartStartExecutionOutcome, StartExecutionError> {
    validate_restart_operation(operations, &request)?;

    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_graph = graph.clone();
    let mut next_jobs = jobs.clone();
    let mut next_job_ids = job_ids.clone();
    let mut next_start_store = start_store.clone();

    let activation = next_services
        .prepare_activation_snapshot(&request.service)
        .map_err(StartExecutionError::ServiceTable)?;
    let operation_deadline_ns = start_operation_deadline_ns(
        next_operations
            .get(request.operation_id)
            .ok_or(OperationStoreError::UnknownOperation {
                id: request.operation_id,
            })
            .map_err(StartExecutionError::OperationStore)?,
        &activation.definition,
        request.started_at_ns,
    );
    let service_transition = next_services
        .transition_service(
            &request.service,
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .map_err(StartExecutionError::ServiceTable)?;

    match evaluate_cacheable_pre_start_checks(
        &next_services,
        &request.service,
        &activation.definition,
    ) {
        PreStartCheckDecision::Passed => {}
        PreStartCheckDecision::Skipped(reason) => {
            let skipped = next_services
                .transition_service(
                    &request.service,
                    ServiceTransition {
                        to: ServiceState::Skipped,
                        cause: reason.cause(),
                    },
                )
                .map_err(StartExecutionError::ServiceTable)?;
            next_services
                .mark_dependent_satisfied_since(&request.service, request.started_at_ns)
                .map_err(StartExecutionError::ServiceTable)?;
            let completed = next_operations
                .complete_operation(
                    request.operation_id,
                    request.started_at_ns,
                    reason.message(),
                )
                .map_err(StartExecutionError::OperationStore)?;

            *services = next_services;
            *operations = next_operations;

            return Ok(RestartStartExecutionOutcome::Terminal(
                RestartStartExecutionTerminalDispatch {
                    service: request.service,
                    operation_id: request.operation_id,
                    outcome: reason.outcome(),
                    operation_events: vec![completed],
                    service_transitions: vec![service_transition, skipped],
                    graph_events: Vec::new(),
                },
            ));
        }
        PreStartCheckDecision::AssertionFailed(check) => {
            // Through apply_start_failure, as every other assert path is. The
            // restart leg used to transition and fail the operation itself, so
            // graph.apply_operation_failed never ran and the failure did not
            // propagate — the one assert path that skipped propagation, and
            // the one where the dependency is most permanently gone, since an
            // AssertionError is never restarted (PEI-370).
            let failure = apply_start_failure(
                &mut next_services,
                &mut next_operations,
                &mut next_graph,
                StartFailureRequest {
                    service: request.service.clone(),
                    operation_id: request.operation_id,
                    failed_at_ns: request.started_at_ns,
                    failure_cause: TransitionCause::AssertionError,
                    reason: format!("AssertionError: {} not satisfied", format_check(&check)),
                    exit_code: None,
                },
            )
            .map_err(StartExecutionError::StartFailure)?;

            *services = next_services;
            *operations = next_operations;
            *graph = next_graph;

            let mut service_transitions = vec![service_transition];
            service_transitions.extend(failure.service_transitions);
            return Ok(RestartStartExecutionOutcome::Terminal(
                RestartStartExecutionTerminalDispatch {
                    service: request.service,
                    operation_id: request.operation_id,
                    outcome: StartPreCheckTerminalOutcome::AssertionFailed {
                        check: format_check(&check),
                    },
                    operation_events: failure.operation_events,
                    service_transitions,
                    graph_events: failure.graph_events,
                },
            ));
        }
        PreStartCheckDecision::RequiresFilesystemHelper { checks } => {
            let pending = PendingPreStartCheck::new(
                request.operation_id,
                request.service.clone(),
                activation,
                checks,
                request.started_at_ns,
                operation_deadline_ns,
                PendingPreStartCheckStart::Restart {
                    resolved_identity: request.resolved_identity,
                    token_summary: request.token_summary,
                },
            );
            let registration = next_start_store.record_pending_pre_start_check(pending);

            *services = next_services;
            *operations = next_operations;
            *start_store = next_start_store;

            return Ok(RestartStartExecutionOutcome::CheckPending(
                RestartStartExecutionCheckPendingDispatch {
                    service: request.service,
                    operation_id: request.operation_id,
                    service_transition,
                    checks: registration.checks,
                    helper_cgroup_id: registration.helper_cgroup_id,
                },
            ));
        }
    }

    let main_job_id = next_job_ids
        .allocate_batch(1, request.started_at_ns)
        .map(|ids| ids[0])
        .map_err(StartExecutionError::JobIdAllocation)?;
    let initial = create_initial_start_job(
        &mut next_jobs,
        &mut next_job_ids,
        &mut next_start_store,
        InitialStartJobRequest {
            service: &request.service,
            operation_id: request.operation_id,
            resolved_identity: request.resolved_identity,
            token_summary: request.token_summary,
            started_at_ns: request.started_at_ns,
            operation_deadline_ns,
            activation: &activation,
            main_job_id,
        },
    )?;

    *services = next_services;
    *operations = next_operations;
    *jobs = next_jobs;
    *job_ids = next_job_ids;
    *start_store = next_start_store;

    Ok(RestartStartExecutionOutcome::Job(Box::new(
        RestartStartExecutionDispatch {
            service: request.service,
            operation_id: request.operation_id,
            job_id: initial.job_id,
            service_transition,
            job_event: initial.job_event,
            job_kind: initial.job_kind,
        },
    )))
}

fn validate_restart_operation(
    operations: &OperationStore,
    request: &RestartStartExecutionRequest,
) -> Result<(), StartExecutionError> {
    let Some(operation) = operations.get(request.operation_id) else {
        return Err(StartExecutionError::OperationStore(
            OperationStoreError::UnknownOperation {
                id: request.operation_id,
            },
        ));
    };
    if operation.operation_type != OperationType::Restart || operation.service != request.service {
        return Err(invalid_restart_start(request, operation.state));
    }
    if operation.state != OperationState::Running {
        return Err(invalid_restart_start(request, operation.state));
    }
    Ok(())
}

fn invalid_restart_start(
    request: &RestartStartExecutionRequest,
    state: OperationState,
) -> StartExecutionError {
    StartExecutionError::OperationStore(OperationStoreError::Transition(
        OperationTransitionError::InvalidTransition {
            id: request.operation_id,
            from: state,
            action: OperationTransitionAction::Start,
        },
    ))
}
