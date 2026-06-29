use crate::job::{JobEvent, JobEventDetail, JobType};
use crate::supervisor::dispatch::{
    SupervisorHealthCheckOutcome, SupervisorHealthCheckTerminalDispatch,
};
use crate::supervisor::state::SupervisorError;

use super::super::HealthCheckError;

pub(in crate::supervisor::health::terminal_apply) fn health_check_succeeded(
    job_event: &JobEvent,
) -> bool {
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

pub(in crate::supervisor::health::terminal_apply) fn validate_health_check_terminal(
    job_event: &JobEvent,
) -> Result<(), SupervisorError> {
    if job_event.job_type != JobType::HealthCheck {
        return Err(SupervisorError::Health(
            HealthCheckError::NotHealthCheckJob {
                job_id: job_event.job_id,
                job_type: job_event.job_type,
            },
        ));
    }
    if !job_event.state.is_terminal() {
        return Err(SupervisorError::Health(
            HealthCheckError::NotTerminalJobEvent {
                job_id: job_event.job_id,
                state: job_event.state,
            },
        ));
    }
    Ok(())
}

pub(in crate::supervisor::health::terminal_apply) fn dispatch(
    job_event: JobEvent,
    outcome: SupervisorHealthCheckOutcome,
    service_transitions: Vec<crate::service::ServiceTableTransition>,
    killed_cgroup_id: String,
) -> SupervisorHealthCheckTerminalDispatch {
    SupervisorHealthCheckTerminalDispatch {
        job_event,
        service_job_event: None,
        outcome,
        service_transitions,
        killed_cgroup_id,
        critical_reboot: None,
    }
}
