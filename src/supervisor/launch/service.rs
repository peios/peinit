use crate::boundary::{Clock, ProcessLauncher, TokenProvider};
use crate::execution::job_started::apply_service_main_job_started;
use crate::execution::launch::{
    LaunchCreatedJobDispatch, LaunchCreatedJobResult,
    launch_created_service_main_job_with_environment,
};
use crate::execution::start::StartReadyContext;

use super::super::dispatch::{SupervisorLaunchDispatch, SupervisorServiceLaunchDispatch};
use super::super::health::apply_health_scheduling_after_transitions;
use super::super::relationships::apply_relationship_reactions_after_transitions;
use super::super::state::{Supervisor, SupervisorError};
use super::super::watchdog::apply_watchdog_scheduling_after_transitions;
use super::super::work::SupervisorWork;
use super::failure::apply_service_launch_failure;
use super::fd_inheritance::inherited_fds_for_job;
use super::record_pending_process_setup;
use crate::execution::launch::LaunchCreatedJobError;
use crate::service::ServiceDefinition;

impl Supervisor {
    pub fn launch_next_pending_job<T, P, C>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
    ) -> Result<Option<SupervisorLaunchDispatch>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
    {
        match self.launch_next_pending_service_job(token_provider, process_launcher, clock)? {
            Some(SupervisorServiceLaunchDispatch::Launched(dispatch)) => Ok(Some(*dispatch)),
            Some(SupervisorServiceLaunchDispatch::Failed(_))
            | Some(SupervisorServiceLaunchDispatch::PendingSetup(_))
            | None => Ok(None),
        }
    }

    pub fn launch_next_pending_service_job<T, P, C>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
    ) -> Result<Option<SupervisorServiceLaunchDispatch>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
    {
        let Some(job_id) = self.pending_launches.front().copied() else {
            return Ok(None);
        };
        let launched_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let request = self.launch_request(job_id, launched_at_ns);

        let mut work = SupervisorWork::from_supervisor(self);
        work.pending_launches.pop_front();

        let global_environment = work.global_environment.clone();
        let launch = service_definition_for_runtime_provisioning(&work, job_id)
            .and_then(|service| {
                process_launcher
                    .provision_service_runtime_directories(&service)
                    .map_err(LaunchCreatedJobError::Boundary)
            })
            .and_then(|()| inherited_fds_for_job(&work, job_id))
            .and_then(|inherited_fds| {
                launch_created_service_main_job_with_environment(
                    &mut work.jobs,
                    token_provider,
                    process_launcher,
                    request,
                    &global_environment,
                    inherited_fds,
                )
            });
        let launch = match launch {
            Ok(LaunchCreatedJobResult::Started(launch)) => *launch,
            Ok(LaunchCreatedJobResult::PendingSetup(setup)) => {
                let pending = record_pending_process_setup(&mut work, setup)?;
                work.commit(self);
                return Ok(Some(SupervisorServiceLaunchDispatch::PendingSetup(pending)));
            }
            Err(error) => {
                let Some(failure) = apply_service_launch_failure(
                    &mut work,
                    job_id,
                    launched_at_ns,
                    error.clone(),
                    self.settings.phase2.max_parallel_starts,
                    self.settings.shutdown.post_kill_timeout_secs,
                )?
                else {
                    return Err(SupervisorError::Launch(error));
                };
                work.commit(self);
                return Ok(Some(SupervisorServiceLaunchDispatch::Failed(Box::new(
                    failure,
                ))));
            }
        };
        let dispatch = apply_started_service_launch(
            &mut work,
            launch,
            launched_at_ns,
            self.settings.phase2.max_parallel_starts,
        )?;

        work.commit(self);

        Ok(Some(dispatch.into()))
    }
}

fn service_definition_for_runtime_provisioning(
    work: &SupervisorWork,
    job_id: crate::ids::JobId,
) -> Result<ServiceDefinition, LaunchCreatedJobError> {
    let job = work
        .jobs
        .get(job_id)
        .cloned()
        .ok_or(LaunchCreatedJobError::JobStore(
            crate::job::JobStoreError::UnknownJob { id: job_id },
        ))?;
    let service = job
        .service
        .ok_or(LaunchCreatedJobError::ServiceMainJobMissingService { job_id })?;
    work.services.definition(&service).cloned().ok_or_else(|| {
        LaunchCreatedJobError::Boundary(crate::boundary::BoundaryError::Process(format!(
            "service definition for {service} is unavailable during runtime directory provisioning"
        )))
    })
}

pub(in crate::supervisor) fn apply_started_service_launch(
    work: &mut SupervisorWork,
    launch: LaunchCreatedJobDispatch,
    launched_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<SupervisorLaunchDispatch, SupervisorError> {
    if let Some(service) = launch.job_event.service.as_deref() {
        work.fd_store.clear_service(service);
    }

    let started = apply_service_main_job_started(
        &mut StartReadyContext {
            services: &mut work.services,
            operations: &mut work.operations,
            graph: &mut work.graph,
            jobs: &mut work.jobs,
            job_ids: &mut work.job_ids,
            start_store: &mut work.start,
        },
        launch.job_event.clone(),
    )
    .map_err(SupervisorError::JobStarted)?;
    if let Some(post_start_hook) = &started.post_start_hook {
        work.queue_created_post_hook_job(post_start_hook);
    }
    if started.post_start_hook.is_none() {
        apply_health_scheduling_after_transitions(
            work,
            &started.service_transitions,
            launched_at_ns,
        );
        apply_watchdog_scheduling_after_transitions(
            work,
            &started.service_transitions,
            launched_at_ns,
        );
    }
    let mut relationship_start_dispatches = apply_relationship_reactions_after_transitions(
        work,
        &started.service_transitions,
        launched_at_ns,
        max_parallel_starts,
    )?;
    remove_readiness_deadlines_for_completed_operations(work, &started.operation_events);
    let mut start_dispatches = work.release_after_graph_events(
        &started.graph_events,
        max_parallel_starts,
        launched_at_ns,
    )?;
    relationship_start_dispatches.append(&mut start_dispatches);

    Ok(SupervisorLaunchDispatch {
        launch,
        started,
        start_dispatches: relationship_start_dispatches,
    })
}

fn remove_readiness_deadlines_for_completed_operations(
    work: &mut SupervisorWork,
    events: &[crate::operation::store::OperationEvent],
) {
    for event in events {
        work.start.remove_readiness_deadline(event.operation_id);
    }
}

impl From<SupervisorLaunchDispatch> for SupervisorServiceLaunchDispatch {
    fn from(dispatch: SupervisorLaunchDispatch) -> Self {
        Self::Launched(Box::new(dispatch))
    }
}
