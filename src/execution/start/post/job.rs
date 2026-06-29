use crate::ids::{JobId, OperationId};
use crate::job::{JobEvent, JobEventDetail, JobExit, JobRecord, JobState, JobStore, JobType};

use super::super::model::StartExecutionError;
use super::super::store::PostStartHookSequence;

pub(super) fn post_start_hook_job(
    sequence: &PostStartHookSequence,
    job_id: JobId,
    argv: Vec<String>,
    hook_index: usize,
    created_at_ns: u64,
) -> Result<JobRecord, StartExecutionError> {
    JobRecord::new_post_exec_hook(
        job_id,
        crate::job::ServiceHookJobSpec {
            service: &sequence.definition,
            argv,
            hook_index: Some(hook_index),
            resolved_identity: sequence.resolved_identity.clone(),
            token_summary: sequence.token_summary.clone(),
            activation_generation: sequence.activation_generation,
            cgroup_generation: sequence.cgroup_generation,
            operation_id: sequence.operation_id,
            created_at_ns,
        },
    )
    .map_err(StartExecutionError::HookJob)
}

pub(super) fn post_start_result(sequence: &PostStartHookSequence) -> String {
    if sequence.had_failure {
        format!(
            "{}; ExecStartPost failure ignored",
            sequence.readiness_result
        )
    } else {
        format!("{}; ExecStartPost completed", sequence.readiness_result)
    }
}

pub(super) fn validate_post_start_hook_terminal_event(
    job_event: &JobEvent,
) -> Result<(), StartExecutionError> {
    if job_event.job_type != JobType::PostExecHook {
        return Err(StartExecutionError::NotPostExecHookJob {
            job_id: job_event.job_id,
            job_type: job_event.job_type,
        });
    }
    if !job_event.state.is_terminal() {
        return Err(StartExecutionError::NotTerminalJobEvent {
            job_id: job_event.job_id,
            state: job_event.state,
        });
    }
    Ok(())
}

pub(super) fn operation_id(job_event: &JobEvent) -> Result<OperationId, StartExecutionError> {
    let service = job_event
        .service
        .clone()
        .ok_or(StartExecutionError::MissingService {
            job_id: job_event.job_id,
        })?;
    job_event
        .operation_id
        .ok_or(StartExecutionError::MissingOperation {
            job_id: job_event.job_id,
            service,
        })
}

pub(super) fn post_start_hook_succeeded(job_event: &JobEvent) -> bool {
    matches!(
        job_event.detail,
        JobEventDetail::Ended {
            exit_code: Some(0),
            exit_signal: None,
            failure_cause: None,
            ..
        }
    )
}

pub(super) fn ended_at_ns(job_event: &JobEvent) -> Result<u64, StartExecutionError> {
    match job_event.detail {
        JobEventDetail::Ended { ended_at_ns, .. } => Ok(ended_at_ns),
        _ => Err(StartExecutionError::NotTerminalJobEvent {
            job_id: job_event.job_id,
            state: job_event.state,
        }),
    }
}

pub(super) fn fail_timed_out_hook(
    jobs: &mut JobStore,
    job_id: JobId,
    now_ns: u64,
) -> Result<JobEvent, StartExecutionError> {
    let state = jobs
        .get(job_id)
        .ok_or(crate::job::JobStoreError::UnknownJob { id: job_id })
        .map_err(StartExecutionError::JobStore)?
        .state;
    match state {
        JobState::Created => jobs
            .fail_job_before_start(job_id, now_ns, "post-start hook timed out before launch")
            .map_err(StartExecutionError::JobStore),
        JobState::Running => jobs
            .fail_running_job(
                job_id,
                now_ns,
                Some(JobExit::Signal(9)),
                "post-start hook timed out",
            )
            .map_err(StartExecutionError::JobStore),
        _ => jobs
            .fail_running_job(
                job_id,
                now_ns,
                Some(JobExit::Signal(9)),
                "post-start hook timed out",
            )
            .map_err(StartExecutionError::JobStore),
    }
}
