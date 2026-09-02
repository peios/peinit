use crate::boundary::{Clock, ProcessLauncher, TokenProvider};
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
    HealthCheckError, fail_launched_health_check_in_work,
};

impl Supervisor {
    /// The controller a health-check launch used to need is gone: the only
    /// thing it did was kill the service's cgroup when a launch failure
    /// escalated, and a probe that never ran no longer escalates (PEI-367).
    pub fn launch_next_pending_health_check_job<T, P, C>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
    ) -> Result<Option<SupervisorHealthCheckLaunchResult>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
    {
        let Some(job_id) = crate::supervisor::pending_queue::next_live_front(
            &mut self.pending_health_launches,
            &self.jobs,
            &mut self.stale_launch_entries,
        ) else {
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
                // A probe that could not be launched says nothing about the
                // service, so it is recorded and left there: no health
                // failure counted, no escalation, and the next interval
                // schedules normally. Escalating here meant a transient authd
                // unavailability could kill a service outright (PEI-367).
                let terminal = fail_launched_health_check_in_work(
                    &mut work,
                    job_id,
                    launched_at_ns,
                    format!("health check launch failed: {error:?}"),
                )?;
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
