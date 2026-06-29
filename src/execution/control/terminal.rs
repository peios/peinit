use crate::boundary::ProcessController;
use crate::job::{JobEvent, JobExit, JobStore};
use crate::operation::operation_timeout_result;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::model::{
    ControlExecutionError, ReloadCommandTerminalDispatch, ReloadCommandTimeoutDispatch,
};
use super::store::{ControlExecutionStore, ReloadCommandDeadline};
use super::terminal_event::{
    ended_at_ns, operation_id, reload_command_failure_reason, reload_command_succeeded, service,
    validate_reload_hook_terminal_event,
};

pub fn complete_reload_command_job(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    control_store: &mut ControlExecutionStore,
    job_event: JobEvent,
) -> Result<ReloadCommandTerminalDispatch, ControlExecutionError> {
    validate_reload_hook_terminal_event(&job_event)?;
    let service = service(&job_event)?;
    let operation_id = operation_id(&job_event, &service)?;
    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_store = control_store.clone();

    next_store.remove_reload_command_deadline(operation_id);
    let ready_received = next_store.take_reload_command_ready(operation_id);
    let service_transition = transition_reloaded_active(&mut next_services, &service)?;
    let ended_at_ns = ended_at_ns(&job_event)?;
    let operation_event = if reload_command_succeeded(&job_event) {
        next_operations
            .complete_operation(
                operation_id,
                ended_at_ns,
                reload_command_success_result(ready_received),
            )
            .map_err(ControlExecutionError::OperationStore)?
    } else {
        next_operations
            .fail_operation(
                operation_id,
                ended_at_ns,
                reload_command_failure_reason(&job_event)?,
            )
            .map_err(ControlExecutionError::OperationStore)?
    };

    *services = next_services;
    *operations = next_operations;
    *control_store = next_store;

    Ok(ReloadCommandTerminalDispatch {
        job_event,
        operation_event,
        service_transition,
    })
}

fn reload_command_success_result(ready_received: bool) -> &'static str {
    if ready_received {
        "reload command confirmed"
    } else {
        "reload command advisory"
    }
}

pub fn timeout_reload_command(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    jobs: &mut JobStore,
    control_store: &mut ControlExecutionStore,
    controller: &mut (impl ProcessController + ?Sized),
    deadline: ReloadCommandDeadline,
    now_ns: u64,
) -> Result<ReloadCommandTimeoutDispatch, ControlExecutionError> {
    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_jobs = jobs.clone();
    let mut next_store = control_store.clone();

    next_store.remove_reload_command_deadline(deadline.operation_id);
    next_store.take_reload_command_ready(deadline.operation_id);
    controller
        .kill_cgroup(&deadline.cgroup_id)
        .map_err(ControlExecutionError::Boundary)?;
    let job_event = next_jobs
        .fail_running_job(
            deadline.job_id,
            now_ns,
            Some(JobExit::Signal(9)),
            "reload command timed out",
        )
        .map_err(ControlExecutionError::JobStore)?;
    let service_transition = transition_reloaded_active(&mut next_services, &deadline.service)?;
    let operation_event = next_operations
        .fail_operation(
            deadline.operation_id,
            now_ns,
            operation_timeout_result("ExecReload command timed out"),
        )
        .map_err(ControlExecutionError::OperationStore)?;

    *services = next_services;
    *operations = next_operations;
    *jobs = next_jobs;
    *control_store = next_store;

    Ok(ReloadCommandTimeoutDispatch {
        job_event,
        operation_event,
        service_transition,
        cgroup_id: deadline.cgroup_id,
        timed_out_at_ns: now_ns,
    })
}

fn transition_reloaded_active(
    services: &mut ServiceTable,
    service: &str,
) -> Result<crate::service::ServiceTableTransition, ControlExecutionError> {
    services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Active,
                cause: TransitionCause::ExplicitReload,
            },
        )
        .map_err(ControlExecutionError::ServiceTable)
}
