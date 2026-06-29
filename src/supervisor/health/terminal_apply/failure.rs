use crate::job::JobEvent;
use crate::service::runtime::{ServiceTransition, TransitionCause};
use crate::service::{RestartEvaluationAction, ServiceDefinition, evaluate_restart_after_failure};
use crate::supervisor::dispatch::{
    SupervisorHealthCheckOutcome, SupervisorHealthCheckTerminalDispatch,
};
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::super::HealthCheckError;
use super::helpers::dispatch;

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(in crate::supervisor::health::terminal_apply) fn apply_health_check_failure(
    work: &mut SupervisorWork,
    job_event: JobEvent,
    cgroup_id: String,
    service: &str,
    definition: &ServiceDefinition,
    observed_at_ns: u64,
) -> Result<SupervisorHealthCheckTerminalDispatch, SupervisorError> {
    let consecutive_failures = work
        .services
        .record_health_failure(service, observed_at_ns)
        .map_err(|error| SupervisorError::Health(HealthCheckError::ServiceTable(error)))?;
    if consecutive_failures < definition.health_check_retries {
        return Ok(dispatch(
            job_event,
            SupervisorHealthCheckOutcome::Unhealthy {
                consecutive_failures,
                retries: definition.health_check_retries,
            },
            Vec::new(),
            cgroup_id,
        ));
    }

    let restart_failures = work
        .services
        .runtime(service)
        .ok_or_else(|| {
            SupervisorError::Health(HealthCheckError::UnknownService {
                service: service.to_string(),
            })
        })?
        .consecutive_restart_failures;
    let evaluation = evaluate_restart_after_failure(
        definition,
        TransitionCause::HealthCheckFailure,
        None,
        restart_failures,
    );
    let transition = match evaluation.action {
        RestartEvaluationAction::Backoff {
            cause, delay_secs, ..
        } => {
            work.health.cancel_service(service);
            let transition = work
                .services
                .transition_service_to_restart_backoff(
                    service,
                    cause,
                    observed_at_ns.saturating_add(delay_secs.saturating_mul(NANOS_PER_SEC)),
                )
                .map_err(|error| SupervisorError::Health(HealthCheckError::ServiceTable(error)))?;
            return Ok(dispatch(
                job_event,
                SupervisorHealthCheckOutcome::RestartScheduled {
                    consecutive_failures,
                    retries: definition.health_check_retries,
                },
                vec![transition],
                cgroup_id,
            ));
        }
        RestartEvaluationAction::Fail { cause, state } => {
            work.health.cancel_service(service);
            work.services
                .transition_service(service, ServiceTransition { to: state, cause })
                .map_err(|error| SupervisorError::Health(HealthCheckError::ServiceTable(error)))?
        }
    };

    Ok(dispatch(
        job_event,
        SupervisorHealthCheckOutcome::Failed {
            consecutive_failures,
            retries: definition.health_check_retries,
        },
        vec![transition],
        cgroup_id,
    ))
}
