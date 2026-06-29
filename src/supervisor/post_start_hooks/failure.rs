use crate::boundary::{BoundaryError, ProcessController, ProcessLaunchError};
use crate::execution::launch::LaunchCreatedJobError;
use crate::ids::JobId;

use super::terminal::{complete_post_start_hook_in_work, schedule_after_final_post_start_hook};
use crate::supervisor::dispatch::SupervisorPostStartHookLaunchFailureDispatch;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

pub(in crate::supervisor) fn apply_post_start_hook_launch_failure<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    job_id: JobId,
    failed_at_ns: u64,
    error: LaunchCreatedJobError,
    max_parallel_starts: u32,
) -> Result<Option<SupervisorPostStartHookLaunchFailureDispatch>, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let Some(reason) = post_start_hook_launch_failure_reason(&error) else {
        return Ok(None);
    };
    let job_event = work
        .jobs
        .fail_job_before_start(job_id, failed_at_ns, reason)
        .map_err(SupervisorError::JobStore)?;
    let terminal = complete_post_start_hook_in_work(work, controller, job_event)?;
    let mut start_dispatches = apply_relationship_reactions_after_transitions(
        work,
        &terminal.service_transitions,
        failed_at_ns,
        max_parallel_starts,
    )?;
    start_dispatches.extend(work.release_after_graph_events(
        &terminal.graph_events,
        max_parallel_starts,
        failed_at_ns,
    )?);
    schedule_after_final_post_start_hook(work, &terminal, failed_at_ns);

    Ok(Some(SupervisorPostStartHookLaunchFailureDispatch {
        terminal,
        start_dispatches,
    }))
}

fn post_start_hook_launch_failure_reason(error: &LaunchCreatedJobError) -> Option<String> {
    let LaunchCreatedJobError::Boundary(error) = error else {
        return None;
    };
    let detail = match error {
        BoundaryError::Token(message) => format!("token materialization failed: {message}"),
        BoundaryError::Process(message) => message.clone(),
        BoundaryError::ProcessLaunch(ProcessLaunchError::ParentSetup { message, .. }) => {
            message.clone()
        }
        BoundaryError::ProcessLaunch(ProcessLaunchError::PreExec { error, .. }) => {
            format!("{} failed with errno {}", error.step.label(), error.errno)
        }
        BoundaryError::ProcessLaunch(ProcessLaunchError::MalformedPreExec { message, .. }) => {
            format!("malformed child setup evidence: {message}")
        }
        _ => return None,
    };
    Some(format!("ExecStartPostFailure: launch failed: {detail}"))
}
