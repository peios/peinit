use crate::boundary::{Clock, ProcessController, ProcessLauncher, TokenProvider};
use crate::execution::launch::{
    LaunchCreatedJobResult, launch_created_pre_exec_hook_job_with_environment,
};

use super::super::dispatch::{SupervisorStartHookLaunchDispatch, SupervisorStartHookLaunchResult};
use super::super::state::{Supervisor, SupervisorError};
use super::super::work::SupervisorWork;
use super::failure::apply_pre_start_hook_launch_failure;
use super::record_pending_process_setup;

impl Supervisor {
    pub fn launch_next_pending_start_hook_job<T, P, C>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
    ) -> Result<Option<SupervisorStartHookLaunchDispatch>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
    {
        let Some(job_id) = self.pending_start_hook_launches.front().copied() else {
            return Ok(None);
        };
        let launched_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let request = self.launch_request(job_id, launched_at_ns);

        let mut work = SupervisorWork::from_supervisor(self);
        work.pending_start_hook_launches.pop_front();

        let launch = launch_created_pre_exec_hook_job_with_environment(
            &mut work.jobs,
            token_provider,
            process_launcher,
            request,
            &work.global_environment,
        )
        .map_err(SupervisorError::Launch)?;

        let launch = match launch {
            LaunchCreatedJobResult::Started(launch) => *launch,
            LaunchCreatedJobResult::PendingSetup(setup) => {
                return Err(SupervisorError::Launch(
                    crate::execution::launch::LaunchCreatedJobError::Boundary(
                        crate::boundary::BoundaryError::Process(format!(
                            "pending setup for start hook job {} requires a process controller",
                            setup.job_id
                        )),
                    ),
                ));
            }
        };

        work.commit(self);

        Ok(Some(SupervisorStartHookLaunchDispatch { launch }))
    }

    pub fn launch_next_pending_start_hook_job_with_controller<T, P, C, R>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
        controller: &mut R,
    ) -> Result<Option<SupervisorStartHookLaunchResult>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
        R: ProcessController + ?Sized,
    {
        let Some(job_id) = self.pending_start_hook_launches.front().copied() else {
            return Ok(None);
        };
        let launched_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let request = self.launch_request(job_id, launched_at_ns);

        let mut work = SupervisorWork::from_supervisor(self);
        work.pending_start_hook_launches.pop_front();

        let launch = launch_created_pre_exec_hook_job_with_environment(
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
                return Ok(Some(SupervisorStartHookLaunchResult::PendingSetup(pending)));
            }
            Err(error) => {
                let Some(failure) = apply_pre_start_hook_launch_failure(
                    &mut work,
                    controller,
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
                return Ok(Some(SupervisorStartHookLaunchResult::Failed(failure)));
            }
        };

        work.commit(self);

        Ok(Some(SupervisorStartHookLaunchResult::Launched(
            SupervisorStartHookLaunchDispatch { launch },
        )))
    }
}
