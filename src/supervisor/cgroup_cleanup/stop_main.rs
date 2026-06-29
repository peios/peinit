use crate::execution::start::{RestartStartExecutionRequest, begin_restart_start_leg};
use crate::ids::OperationId;
use crate::operation::{OperationType, operation_timeout_result};
use crate::security::TokenSummary;
use crate::service::runtime::{LeakedCgroupKind, ServiceState, ServiceTransition, TransitionCause};

use crate::supervisor::fd_store_lifecycle::explicit_stop_record_clears_fd_store;
use crate::supervisor::health::apply_health_scheduling_after_transitions;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::watchdog::apply_watchdog_scheduling_after_transitions;
use crate::supervisor::work::SupervisorWork;

const STOP_POST_KILL_TIMEOUT_RESULT: &str = "service cgroup remained populated after SIGKILL";

pub(super) fn apply_stop_main_abandoned(
    work: &mut SupervisorWork,
    service: &str,
    root_cgroup_id: String,
    operation_id: OperationId,
    now_ns: u64,
    max_parallel_starts: u32,
) -> Result<(), SupervisorError> {
    let Some(runtime) = work.services.runtime(service) else {
        return Ok(());
    };
    if runtime.state != ServiceState::Stopping {
        return Ok(());
    }
    let should_clear_fd_store = work
        .operations
        .get(operation_id)
        .is_some_and(explicit_stop_record_clears_fd_store);
    work.services
        .record_leaked_cgroup(
            service,
            root_cgroup_id,
            LeakedCgroupKind::ServiceTree,
            now_ns,
        )
        .map_err(|error| {
            SupervisorError::Control(
                crate::execution::control::ControlExecutionError::ServiceTable(error),
            )
        })?;
    let transition = work
        .services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Abandoned,
                cause: TransitionCause::ProcessUnkillable,
            },
        )
        .map_err(|error| {
            SupervisorError::Control(
                crate::execution::control::ControlExecutionError::ServiceTable(error),
            )
        })?;
    work.control.take_restart_stop_leg(operation_id);
    if work
        .operations
        .get(operation_id)
        .is_some_and(|operation| !operation.state.is_terminal())
    {
        work.operations
            .fail_operation(
                operation_id,
                now_ns,
                operation_timeout_result(STOP_POST_KILL_TIMEOUT_RESULT),
            )
            .map_err(|error| {
                SupervisorError::Control(
                    crate::execution::control::ControlExecutionError::OperationStore(error),
                )
            })?;
    }
    if should_clear_fd_store {
        work.fd_store.clear_service(service);
    }
    apply_health_scheduling_after_transitions(work, std::slice::from_ref(&transition), now_ns);
    apply_watchdog_scheduling_after_transitions(work, std::slice::from_ref(&transition), now_ns);
    apply_relationship_reactions_after_transitions(
        work,
        &[transition],
        now_ns,
        max_parallel_starts,
    )?;
    Ok(())
}

pub(super) fn apply_stop_main_empty(
    work: &mut SupervisorWork,
    service: &str,
    operation_id: OperationId,
    now_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<crate::execution::start::RestartStartExecutionDispatch>, SupervisorError> {
    let Some(runtime) = work.services.runtime(service) else {
        return Ok(Vec::new());
    };
    if runtime.state != ServiceState::Stopping {
        return Ok(Vec::new());
    }
    let operation = work.operations.get(operation_id).cloned();
    let transition = work
        .services
        .transition_service(
            service,
            ServiceTransition {
                to: stopped_state(runtime.cause),
                cause: stopped_cause(runtime.cause),
            },
        )
        .map_err(|error| {
            SupervisorError::Control(
                crate::execution::control::ControlExecutionError::ServiceTable(error),
            )
        })?;
    apply_health_scheduling_after_transitions(work, std::slice::from_ref(&transition), now_ns);
    apply_watchdog_scheduling_after_transitions(work, std::slice::from_ref(&transition), now_ns);
    apply_relationship_reactions_after_transitions(
        work,
        std::slice::from_ref(&transition),
        now_ns,
        max_parallel_starts,
    )?;

    let Some(operation) = operation else {
        return Ok(Vec::new());
    };
    match operation.operation_type {
        OperationType::Stop => {
            if !operation.state.is_terminal() {
                work.operations
                    .complete_operation(operation_id, now_ns, "inactive")
                    .map_err(|error| {
                        SupervisorError::Control(
                            crate::execution::control::ControlExecutionError::OperationStore(error),
                        )
                    })?;
            }
            if explicit_stop_record_clears_fd_store(&operation) {
                work.fd_store.clear_service(service);
            }
            Ok(Vec::new())
        }
        OperationType::Restart => {
            begin_restart_start_after_post_kill(work, service, operation_id, now_ns)
        }
        _ => Ok(Vec::new()),
    }
}

fn begin_restart_start_after_post_kill(
    work: &mut SupervisorWork,
    service: &str,
    operation_id: OperationId,
    now_ns: u64,
) -> Result<Vec<crate::execution::start::RestartStartExecutionDispatch>, SupervisorError> {
    if work.control.take_restart_stop_leg(operation_id).is_none() {
        return Ok(Vec::new());
    }
    let definition = work.services.definition(service).ok_or_else(|| {
        SupervisorError::MissingStartCredentials {
            service: service.to_string(),
        }
    })?;
    let resolved_identity = definition.identity.clone();
    let dispatch = begin_restart_start_leg(
        &mut work.services,
        &mut work.operations,
        &mut work.jobs,
        &mut work.job_ids,
        &mut work.start,
        RestartStartExecutionRequest {
            service: service.to_string(),
            operation_id,
            resolved_identity: resolved_identity.clone(),
            token_summary: TokenSummary::requested_identity(resolved_identity),
            started_at_ns: now_ns,
        },
    )
    .map_err(SupervisorError::Start)?;

    Ok(match dispatch {
        crate::execution::start::RestartStartExecutionOutcome::Job(dispatch) => vec![*dispatch],
        crate::execution::start::RestartStartExecutionOutcome::Terminal(_)
        | crate::execution::start::RestartStartExecutionOutcome::CheckPending(_) => Vec::new(),
    })
}

fn stopped_state(cause: Option<TransitionCause>) -> ServiceState {
    match cause {
        Some(TransitionCause::ConflictEviction | TransitionCause::BindsToPropagation) => {
            ServiceState::Failed
        }
        _ => ServiceState::Inactive,
    }
}

fn stopped_cause(cause: Option<TransitionCause>) -> TransitionCause {
    match cause {
        Some(
            cause @ (TransitionCause::ConflictEviction
            | TransitionCause::BindsToPropagation
            | TransitionCause::ShutdownWave),
        ) => cause,
        _ => TransitionCause::ExplicitStop,
    }
}
