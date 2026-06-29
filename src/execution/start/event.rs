use crate::job::{JobEvent, JobEventDetail, JobType};

use super::model::StartExecutionError;

pub(super) fn validate_pre_start_hook_terminal_event(
    job_event: &JobEvent,
) -> Result<(), StartExecutionError> {
    if job_event.job_type != JobType::PreExecHook {
        return Err(StartExecutionError::NotPreExecHookJob {
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

pub(super) fn service(job_event: &JobEvent) -> Result<String, StartExecutionError> {
    job_event
        .service
        .clone()
        .ok_or(StartExecutionError::MissingService {
            job_id: job_event.job_id,
        })
}

pub(super) fn operation_id(
    job_event: &JobEvent,
    service: &str,
) -> Result<crate::ids::OperationId, StartExecutionError> {
    job_event
        .operation_id
        .ok_or_else(|| StartExecutionError::MissingOperation {
            job_id: job_event.job_id,
            service: service.to_string(),
        })
}

pub(super) fn pre_start_hook_succeeded(job_event: &JobEvent) -> bool {
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

pub(super) fn pre_start_hook_failure_reason(
    job_event: &JobEvent,
) -> Result<String, StartExecutionError> {
    match &job_event.detail {
        JobEventDetail::Ended {
            exit_code: Some(code),
            ..
        } => Ok(format!("ExecStartPre command failed (exit {code})")),
        JobEventDetail::Ended {
            exit_signal: Some(signal),
            ..
        } => Ok(format!("ExecStartPre command failed (signal {signal})")),
        JobEventDetail::Ended {
            failure_cause: Some(cause),
            ..
        } => Ok(format!("ExecStartPre command failed ({cause})")),
        JobEventDetail::Ended { .. } => Ok("ExecStartPre command failed".to_string()),
        _ => Err(StartExecutionError::NotTerminalJobEvent {
            job_id: job_event.job_id,
            state: job_event.state,
        }),
    }
}
