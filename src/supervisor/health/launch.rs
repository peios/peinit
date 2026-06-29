use crate::boundary::{Clock, ProcessController, ProcessLauncher, TokenProvider};
use crate::execution::launch::{
    LaunchCreatedJobDispatch, LaunchCreatedJobResult,
    launch_created_health_check_job_with_environment,
};
use crate::job::JobStoreError;
use crate::supervisor::dispatch::{
    SupervisorHealthCheckLaunchCancelledDispatch, SupervisorHealthCheckLaunchDispatch,
    SupervisorHealthCheckLaunchFailureDispatch, SupervisorHealthCheckLaunchResult,
};
use crate::supervisor::launch::record_pending_process_setup;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

use super::{
    HealthCheckError, fail_created_health_check_in_work, terminate_service_after_health_escalation,
};

impl Supervisor {
    pub fn launch_next_pending_health_check_job<T, P, C, R>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
        controller: &mut R,
    ) -> Result<Option<SupervisorHealthCheckLaunchResult>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
        R: ProcessController + ?Sized,
    {
        let Some(job_id) = self.pending_health_launches.front().copied() else {
            return Ok(None);
        };
        let launched_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let request = self.launch_request(job_id, launched_at_ns);

        let mut work = SupervisorWork::from_supervisor(self);
        work.pending_health_launches.pop_front();

        if stale_pending_health_check(&work, job_id) {
            let job_event = work
                .jobs
                .fail_job_before_start(job_id, launched_at_ns, "health check cancelled")
                .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
            work.commit(self);
            return Ok(Some(SupervisorHealthCheckLaunchResult::Cancelled(
                SupervisorHealthCheckLaunchCancelledDispatch {
                    job_event,
                    reason: "health check cancelled".to_string(),
                },
            )));
        }

        let launch = launch_created_health_check_job_with_environment(
            &mut work.jobs,
            token_provider,
            process_launcher,
            request,
            &work.global_environment,
        );
        let launch = match launch {
            Ok(LaunchCreatedJobResult::Started(launch)) => *launch,
            Ok(LaunchCreatedJobResult::PendingSetup(setup)) => {
                let pending = record_pending_process_setup(&mut work, setup)?;
                work.commit(self);
                return Ok(Some(SupervisorHealthCheckLaunchResult::PendingSetup(
                    pending,
                )));
            }
            Err(error) => {
                let job = work
                    .jobs
                    .get(job_id)
                    .cloned()
                    .ok_or(JobStoreError::UnknownJob { id: job_id })
                    .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
                let mut terminal = fail_created_health_check_in_work(
                    &mut work,
                    job_id,
                    launched_at_ns,
                    format!("health check launch failed: {error:?}"),
                )?;
                if let Some(service) = job.service.as_deref() {
                    terminate_service_after_health_escalation(
                        &mut work,
                        &mut terminal,
                        service,
                        job.cgroup_generation,
                        controller,
                        launched_at_ns,
                        self.settings.shutdown.post_kill_timeout_secs,
                    )?;
                }
                work.commit(self);
                return Ok(Some(SupervisorHealthCheckLaunchResult::Failed(Box::new(
                    SupervisorHealthCheckLaunchFailureDispatch { terminal },
                ))));
            }
        };

        mark_health_invocation_running(&mut work, job_id, launched_at_ns)?;
        work.commit(self);

        Ok(Some(SupervisorHealthCheckLaunchResult::Launched(
            SupervisorHealthCheckLaunchDispatch { launch },
        )))
    }
}

pub(in crate::supervisor) fn apply_started_health_check_launch(
    work: &mut SupervisorWork,
    launch: LaunchCreatedJobDispatch,
    launched_at_ns: u64,
) -> Result<SupervisorHealthCheckLaunchDispatch, SupervisorError> {
    mark_health_invocation_running(work, launch.job_id, launched_at_ns)?;
    Ok(SupervisorHealthCheckLaunchDispatch { launch })
}

fn mark_health_invocation_running(
    work: &mut SupervisorWork,
    job_id: crate::ids::JobId,
    launched_at_ns: u64,
) -> Result<(), SupervisorError> {
    let timeout_secs = work
        .jobs
        .get(job_id)
        .and_then(|job| job.service.as_deref())
        .and_then(|service| work.services.definition(service))
        .map(|definition| definition.health_check_timeout_secs)
        .ok_or(SupervisorError::Health(HealthCheckError::JobStore(
            JobStoreError::UnknownJob { id: job_id },
        )))?;
    work.health.mark_invocation_running(
        job_id,
        launched_at_ns.saturating_add(timeout_secs.saturating_mul(1_000_000_000)),
    );
    Ok(())
}

fn stale_pending_health_check(work: &SupervisorWork, job_id: crate::ids::JobId) -> bool {
    let Some(job) = work.jobs.get(job_id) else {
        return true;
    };
    let Some(service) = job.service.as_deref() else {
        return true;
    };
    !work
        .services
        .runtime(service)
        .is_some_and(|runtime| runtime.state == crate::service::runtime::ServiceState::Active)
        || !work
            .health
            .has_invocation(service, job.activation_generation)
}
