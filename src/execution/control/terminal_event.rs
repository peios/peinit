use crate::ids::OperationId;
use crate::job::{JobEvent, JobEventDetail, JobType};

use super::model::ControlExecutionError;

pub(super) fn validate_reload_hook_terminal_event(
    job_event: &JobEvent,
) -> Result<(), ControlExecutionError> {
    if job_event.job_type != JobType::ReloadHook {
        return Err(ControlExecutionError::NotReloadHookJob {
            job_id: job_event.job_id,
            job_type: job_event.job_type,
        });
    }
    if !job_event.state.is_terminal() {
        return Err(ControlExecutionError::NotTerminalJobEvent {
            job_id: job_event.job_id,
            state: job_event.state,
        });
    }
    Ok(())
}

pub(super) fn service(job_event: &JobEvent) -> Result<String, ControlExecutionError> {
    job_event
        .service
        .clone()
        .ok_or(ControlExecutionError::MissingService {
            job_id: job_event.job_id,
        })
}

pub(super) fn operation_id(
    job_event: &JobEvent,
    service: &str,
) -> Result<OperationId, ControlExecutionError> {
    job_event
        .operation_id
        .ok_or_else(|| ControlExecutionError::MissingOperation {
            job_id: job_event.job_id,
            service: service.to_string(),
        })
}

pub(super) fn reload_command_succeeded(job_event: &JobEvent) -> bool {
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

pub(super) fn ended_at_ns(job_event: &JobEvent) -> Result<u64, ControlExecutionError> {
    match job_event.detail {
        JobEventDetail::Ended { ended_at_ns, .. } => Ok(ended_at_ns),
        _ => Err(ControlExecutionError::NotTerminalJobEvent {
            job_id: job_event.job_id,
            state: job_event.state,
        }),
    }
}

pub(super) fn reload_command_failure_reason(
    job_event: &JobEvent,
) -> Result<String, ControlExecutionError> {
    match &job_event.detail {
        JobEventDetail::Ended {
            exit_code: Some(code),
            ..
        } => Ok(format!("ExecReload command failed (exit {code})")),
        JobEventDetail::Ended {
            exit_signal: Some(signal),
            ..
        } => Ok(format!("ExecReload command failed (signal {signal})")),
        JobEventDetail::Ended {
            failure_cause: Some(cause),
            ..
        } => Ok(format!("ExecReload command failed ({cause})")),
        JobEventDetail::Ended { .. } => Ok("ExecReload command failed".to_string()),
        _ => Err(ControlExecutionError::NotTerminalJobEvent {
            job_id: job_event.job_id,
            state: job_event.state,
        }),
    }
}
