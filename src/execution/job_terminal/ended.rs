use crate::job::{JobEvent, JobEventDetail, JobType};
use crate::service::{ServiceDefinition, is_success_exit_code};

use super::model::ServiceMainJobTerminalError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EndedJob {
    pub(super) ended_at_ns: u64,
    pub(super) exit_code: Option<i32>,
    exit_signal: Option<i32>,
    failure_cause: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TerminalServiceMainEvent {
    pub(super) service: String,
    pub(super) ended: EndedJob,
}

impl EndedJob {
    pub(super) fn is_success_for(&self, definition: &ServiceDefinition) -> bool {
        self.exit_signal.is_none()
            && self.failure_cause.is_none()
            && self
                .exit_code
                .is_some_and(|code| is_success_exit_code(definition, code))
    }

    pub(super) fn result(&self) -> String {
        match self.exit_code {
            Some(code) => format!("exit code {code}"),
            None => "completed".to_string(),
        }
    }

    pub(super) fn failure_reason(&self) -> String {
        if let Some(cause) = &self.failure_cause {
            return cause.clone();
        }
        if let Some(signal) = self.exit_signal {
            return format!("ProcessCrash: signal {signal}");
        }
        if let Some(code) = self.exit_code {
            return format!("ProcessCrash: exit code {code}");
        }
        "ProcessCrash: service-main job ended without exit status".to_string()
    }
}

pub(super) fn validate_terminal_service_main_event(
    event: &JobEvent,
) -> Result<TerminalServiceMainEvent, ServiceMainJobTerminalError> {
    if event.job_type != JobType::ServiceMain {
        return Err(ServiceMainJobTerminalError::NotServiceMainJob {
            job_id: event.job_id,
            job_type: event.job_type,
        });
    }
    let Some(service) = event.service.clone() else {
        return Err(ServiceMainJobTerminalError::MissingService {
            job_id: event.job_id,
        });
    };
    let JobEventDetail::Ended {
        ended_at_ns,
        exit_code,
        exit_signal,
        failure_cause,
        ..
    } = &event.detail
    else {
        return Err(ServiceMainJobTerminalError::NotTerminalEvent {
            job_id: event.job_id,
        });
    };
    Ok(TerminalServiceMainEvent {
        service,
        ended: EndedJob {
            ended_at_ns: *ended_at_ns,
            exit_code: *exit_code,
            exit_signal: *exit_signal,
            failure_cause: failure_cause.clone(),
        },
    })
}
