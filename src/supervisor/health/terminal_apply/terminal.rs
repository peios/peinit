use crate::job::JobEvent;
use crate::service::runtime::ServiceState;
use crate::supervisor::dispatch::{
    SupervisorHealthCheckOutcome, SupervisorHealthCheckTerminalDispatch,
};
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::super::HealthCheckError;
use super::failure::apply_health_check_failure;
use super::helpers::{dispatch, health_check_succeeded, validate_health_check_terminal};

pub(in crate::supervisor) fn apply_health_check_terminal_in_work(
    work: &mut SupervisorWork,
    job_event: JobEvent,
    cgroup_id: String,
    observed_at_ns: u64,
) -> Result<SupervisorHealthCheckTerminalDispatch, SupervisorError> {
    validate_health_check_terminal(&job_event)?;
    let Some(service) = job_event.service.clone() else {
        return Err(SupervisorError::Health(HealthCheckError::MissingService {
            job_id: job_event.job_id,
        }));
    };
    let invocation = work.health.remove_invocation(job_event.job_id);
    if invocation.is_none() {
        return Ok(stale_dispatch(job_event, cgroup_id));
    }
    let Some(definition) = work.services.definition(&service).cloned() else {
        return Ok(stale_dispatch(job_event, cgroup_id));
    };
    let Some(runtime) = work.services.runtime(&service).cloned() else {
        return Ok(stale_dispatch(job_event, cgroup_id));
    };
    if runtime.state != ServiceState::Active {
        return Ok(stale_dispatch(job_event, cgroup_id));
    }

    if health_check_succeeded(&job_event) {
        work.services
            .record_health_success(&service, observed_at_ns)
            .map_err(|error| SupervisorError::Health(HealthCheckError::ServiceTable(error)))?;
        return Ok(dispatch(
            job_event,
            SupervisorHealthCheckOutcome::Healthy,
            Vec::new(),
            cgroup_id,
        ));
    }

    apply_health_check_failure(
        work,
        job_event,
        cgroup_id,
        &service,
        &definition,
        observed_at_ns,
    )
}

fn stale_dispatch(job_event: JobEvent, cgroup_id: String) -> SupervisorHealthCheckTerminalDispatch {
    dispatch(
        job_event,
        SupervisorHealthCheckOutcome::Stale,
        Vec::new(),
        cgroup_id,
    )
}
