use crate::execution::job_terminal::ServiceMainJobTerminalDispatch;
use crate::job::{JobEvent, JobExit, JobState};
use crate::operation::{OperationState, OperationType};
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::SupervisorError;
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::work::SupervisorWork;

use super::super::fd_store_lifecycle::explicit_stop_event_clears_fd_store;

pub(super) struct NoReloadCleanupController;

pub(super) trait ReloadCleanupController {
    fn kill_reload_cgroup(
        &mut self,
        cgroup_id: &str,
    ) -> Result<bool, crate::boundary::BoundaryError>;
}

impl ReloadCleanupController for NoReloadCleanupController {
    fn kill_reload_cgroup(
        &mut self,
        _cgroup_id: &str,
    ) -> Result<bool, crate::boundary::BoundaryError> {
        Ok(false)
    }
}

impl<T> ReloadCleanupController for T
where
    T: crate::boundary::ProcessController + ?Sized,
{
    fn kill_reload_cgroup(
        &mut self,
        cgroup_id: &str,
    ) -> Result<bool, crate::boundary::BoundaryError> {
        self.kill_cgroup(cgroup_id)?;
        Ok(true)
    }
}

pub(super) fn clear_fd_store_after_explicit_stop(
    work: &mut SupervisorWork,
    dispatch: &ServiceMainJobTerminalDispatch,
) {
    for event in &dispatch.operation_events {
        if explicit_stop_event_clears_fd_store(event) {
            work.fd_store.clear_service(&event.service);
        }
    }
}

pub(super) fn remove_satisfied_stop_deadlines(
    work: &mut SupervisorWork,
    dispatch: &ServiceMainJobTerminalDispatch,
) {
    for event in &dispatch.operation_events {
        work.control.remove_stop_timeout(event.operation_id);
    }
}

pub(super) fn remove_satisfied_readiness_deadlines(
    work: &mut SupervisorWork,
    dispatch: &ServiceMainJobTerminalDispatch,
) {
    for event in &dispatch.operation_events {
        work.start.remove_readiness_deadline(event.operation_id);
    }
}

pub(super) fn cancel_reload_after_main_exit<C>(
    work: &mut SupervisorWork,
    dispatch: &mut ServiceMainJobTerminalDispatch,
    controller: &mut C,
    observed_at_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<Vec<JobEvent>, SupervisorError>
where
    C: ReloadCleanupController + ?Sized,
{
    let services = dispatch
        .service_transitions
        .iter()
        .filter(|transition| {
            transition.event.from == ServiceState::Reloading
                && transition.event.cause == TransitionCause::ProcessCrash
        })
        .map(|transition| transition.event.service.clone())
        .collect::<Vec<_>>();

    let mut cleanup_job_events = Vec::new();
    for service in services {
        let cancelled = work.control.cancel_reload_for_service(&service);
        for deadline in cancelled.detections {
            if let Some(event) = fail_running_reload_operation(
                work,
                deadline.operation_id,
                observed_at_ns,
                "reload failed: main process exited during reload",
            )? {
                dispatch.operation_events.push(event);
            }
        }
        for deadline in cancelled.commands {
            if let Some(event) = fail_running_reload_operation(
                work,
                deadline.operation_id,
                observed_at_ns,
                "reload failed: main process exited during reload",
            )? {
                dispatch.operation_events.push(event);
            }
            if let Some(event) = fail_reload_command_job(
                work,
                controller,
                &service,
                deadline.job_id,
                &deadline.cgroup_id,
                observed_at_ns,
                post_kill_timeout_secs,
            )? {
                cleanup_job_events.push(event);
            }
        }
    }
    Ok(cleanup_job_events)
}

fn fail_running_reload_operation(
    work: &mut SupervisorWork,
    operation_id: crate::ids::OperationId,
    observed_at_ns: u64,
    reason: &'static str,
) -> Result<Option<crate::operation::store::OperationEvent>, SupervisorError> {
    let Some(operation) = work.operations.get(operation_id) else {
        return Ok(None);
    };
    if operation.operation_type != OperationType::Reload
        || operation.state != OperationState::Running
    {
        return Ok(None);
    }
    work.operations
        .fail_operation(operation_id, observed_at_ns, reason)
        .map(Some)
        .map_err(|error| {
            SupervisorError::Control(
                crate::execution::control::ControlExecutionError::OperationStore(error),
            )
        })
}

fn fail_reload_command_job(
    work: &mut SupervisorWork,
    controller: &mut (impl ReloadCleanupController + ?Sized),
    service: &str,
    job_id: crate::ids::JobId,
    cgroup_id: &str,
    observed_at_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<Option<JobEvent>, SupervisorError> {
    let Some(job) = work.jobs.get(job_id).cloned() else {
        return Ok(None);
    };
    match job.state {
        JobState::Created => work
            .jobs
            .fail_job_before_start(
                job_id,
                observed_at_ns,
                "reload command cancelled because service main exited during reload",
            )
            .map(Some)
            .map_err(SupervisorError::JobStore),
        JobState::Running => fail_running_reload_command_job(
            work,
            controller,
            service,
            job_id,
            cgroup_id,
            observed_at_ns,
            post_kill_timeout_secs,
        ),
        _ => Ok(None),
    }
}

fn fail_running_reload_command_job(
    work: &mut SupervisorWork,
    controller: &mut (impl ReloadCleanupController + ?Sized),
    service: &str,
    job_id: crate::ids::JobId,
    cgroup_id: &str,
    observed_at_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<Option<JobEvent>, SupervisorError> {
    let killed = controller.kill_reload_cgroup(cgroup_id).map_err(|error| {
        SupervisorError::Control(crate::execution::control::ControlExecutionError::Boundary(
            error,
        ))
    })?;
    if killed {
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            service,
            cgroup_id,
            CgroupCleanupKind::Hooks,
            observed_at_ns,
            post_kill_timeout_secs,
        );
        return work
            .jobs
            .fail_running_job(
                job_id,
                observed_at_ns,
                Some(JobExit::Signal(9)),
                "reload command killed because service main exited during reload",
            )
            .map(Some)
            .map_err(SupervisorError::JobStore);
    }
    work.jobs
        .fail_running_job(
            job_id,
            observed_at_ns,
            None,
            "reload command cancelled because service main exited during reload",
        )
        .map(Some)
        .map_err(SupervisorError::JobStore)
}
