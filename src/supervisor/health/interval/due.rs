use crate::execution::command::parse_executable_command;
use crate::job::{JobRecord, ServiceHealthCheckJobSpec};
use crate::security::TokenSummary;
use crate::service::ServiceDefinition;
use crate::service::runtime::ServiceState;
use crate::supervisor::dispatch::{
    SupervisorHealthCheckIntervalAction, SupervisorHealthCheckIntervalDispatch,
};
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::super::{HealthCheckError, HealthCheckIntervalDeadline};
use super::scheduling::{health_enabled, seconds_to_ns};

pub(super) fn process_due_health_check_interval(
    work: &mut SupervisorWork,
    deadline: HealthCheckIntervalDeadline,
    now_ns: u64,
) -> Result<SupervisorHealthCheckIntervalDispatch, SupervisorError> {
    let service = deadline.service.clone();
    let Some(definition) = work.services.definition(&service).cloned() else {
        work.health.cancel_service(&service);
        return Ok(dispatch(
            &service,
            SupervisorHealthCheckIntervalAction::NotConfigured,
        ));
    };
    let Some(runtime) = work.services.runtime(&service).cloned() else {
        work.health.cancel_service(&service);
        return Ok(dispatch(
            &service,
            SupervisorHealthCheckIntervalAction::NotConfigured,
        ));
    };
    if runtime.generation != deadline.activation_generation || runtime.state != ServiceState::Active
    {
        work.health.cancel_service(&service);
        return Ok(dispatch(
            &service,
            SupervisorHealthCheckIntervalAction::SkippedState {
                state: runtime.state,
            },
        ));
    }
    if !health_enabled(&definition) {
        work.health.cancel_service(&service);
        return Ok(dispatch(
            &service,
            SupervisorHealthCheckIntervalAction::NotConfigured,
        ));
    }

    work.health.schedule_interval(
        &service,
        deadline.activation_generation,
        deadline.cgroup_generation,
        now_ns.saturating_add(seconds_to_ns(definition.health_check_interval_secs)),
    );

    if work
        .health
        .has_invocation(&service, deadline.activation_generation)
    {
        let job_id = work
            .jobs
            .active_for_service(&service)
            .into_iter()
            .find(|job_id| {
                work.jobs
                    .get(*job_id)
                    .is_some_and(|job| job.job_type == crate::job::JobType::HealthCheck)
            });
        return Ok(dispatch(
            &service,
            SupervisorHealthCheckIntervalAction::SkippedOverlap { job_id },
        ));
    }

    let job_event = create_health_check_job(
        work,
        &definition,
        deadline.activation_generation,
        deadline.cgroup_generation,
        now_ns,
    )?;
    work.queue_created_health_check_job(&job_event);
    Ok(dispatch(
        &service,
        SupervisorHealthCheckIntervalAction::Created {
            job_event: Box::new(job_event),
        },
    ))
}

fn create_health_check_job(
    work: &mut SupervisorWork,
    definition: &ServiceDefinition,
    activation_generation: u64,
    cgroup_generation: u64,
    now_ns: u64,
) -> Result<crate::job::JobEvent, SupervisorError> {
    let command = definition.health_check.clone().ok_or_else(|| {
        SupervisorError::Health(HealthCheckError::UnknownService {
            service: definition.name.clone(),
        })
    })?;
    let argv = parse_executable_command(&command).map_err(|source| {
        SupervisorError::Health(HealthCheckError::InvalidCommand {
            service: definition.name.clone(),
            command: command.clone(),
            source,
        })
    })?;
    let job_id = work
        .job_ids
        .allocate_batch(1, now_ns)
        .map(|ids| ids[0])
        .map_err(|error| SupervisorError::Health(HealthCheckError::JobIdAllocation(error)))?;
    let resolved_identity = definition.identity.clone();
    let job = JobRecord::new_health_check(
        job_id,
        ServiceHealthCheckJobSpec {
            service: definition,
            argv,
            resolved_identity: resolved_identity.clone(),
            token_summary: TokenSummary::requested_identity(resolved_identity),
            activation_generation,
            cgroup_generation,
            created_at_ns: now_ns,
        },
    )
    .map_err(|error| SupervisorError::Health(HealthCheckError::JobBuild(error)))?;
    let health_cgroup_id = job.cgroup_id.clone();
    let job_event = work
        .jobs
        .create_job(job)
        .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
    work.health.record_pending_invocation(
        &definition.name,
        activation_generation,
        job_id,
        health_cgroup_id,
    );
    Ok(job_event)
}

fn dispatch(
    service: &str,
    action: SupervisorHealthCheckIntervalAction,
) -> SupervisorHealthCheckIntervalDispatch {
    SupervisorHealthCheckIntervalDispatch {
        service: service.to_string(),
        action,
    }
}
