use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::job::{JobEvent, JobExit, JobState, JobStore};
use crate::operation::operation_timeout_result;
use crate::service::runtime::TransitionCause;

use crate::execution::start::{
    ReadinessDeadline, ReadinessTimeoutDispatch, StartExecutionContext, StartExecutionError,
};

pub fn timeout_readiness<P>(
    context: &mut StartExecutionContext<'_, P>,
    deadline: ReadinessDeadline,
    now_ns: u64,
) -> Result<ReadinessTimeoutDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_store = context.start_store.clone();

    next_store.remove_readiness_deadline(deadline.operation_id);
    context
        .controller
        .kill_cgroup(&deadline.service_cgroup_id)
        .map_err(StartExecutionError::Boundary)?;
    let job_event = fail_killed_main_job(&mut next_jobs, &deadline, now_ns)?;
    let failure = apply_start_failure(
        &mut next_services,
        &mut next_operations,
        &mut next_graph,
        StartFailureRequest {
            service: deadline.service,
            operation_id: deadline.operation_id,
            failed_at_ns: now_ns,
            failure_cause: TransitionCause::ReadinessTimeout,
            reason: operation_timeout_result("start timed out waiting for READY=1"),
            exit_code: None,
        },
    )
    .map_err(StartExecutionError::StartFailure)?;

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.jobs = next_jobs;
    *context.start_store = next_store;

    Ok(ReadinessTimeoutDispatch {
        job_event,
        operation_events: failure.operation_events,
        service_transitions: failure.service_transitions,
        graph_events: failure.graph_events,
        killed_cgroup_id: deadline.service_cgroup_id,
        timed_out_at_ns: now_ns,
    })
}

/// Retire the main job the timeout just killed, as the watchdog and health
/// escalation paths do.
///
/// The timeout is the failure; the process's exit is a consequence of the
/// SIGKILL above. Left running, the job was reaped later as a fresh failure
/// and the restart budget was charged twice per timeout: a ReadinessTimeout
/// loop with RestartMaxRetries=4 got three activations where a ProcessCrash
/// loop got five (PEI-822). A job that was not yet running -- still queued
/// for launch, or awaiting its setup status -- has no process to retire here.
fn fail_killed_main_job(
    jobs: &mut JobStore,
    deadline: &ReadinessDeadline,
    now_ns: u64,
) -> Result<Option<JobEvent>, StartExecutionError> {
    let running = jobs
        .get(deadline.job_id)
        .is_some_and(|job| job.state == JobState::Running);
    if !running {
        return Ok(None);
    }
    jobs.fail_running_job(
        deadline.job_id,
        now_ns,
        Some(JobExit::Signal(libc::SIGKILL)),
        "readiness timed out",
    )
    .map(Some)
    .map_err(StartExecutionError::JobStore)
}
