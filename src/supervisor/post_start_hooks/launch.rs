use crate::boundary::{Clock, ProcessController, ProcessLauncher, TokenProvider};
use crate::execution::launch::{
    LaunchCreatedJobResult, launch_created_post_exec_hook_job_with_environment,
};

use super::failure::apply_post_start_hook_launch_failure;
use crate::supervisor::dispatch::{
    SupervisorPostStartHookLaunchDispatch, SupervisorPostStartHookLaunchResult,
};
use crate::supervisor::launch::record_pending_process_setup;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn launch_next_pending_post_hook_job_with_controller<T, P, C, R>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
        controller: &mut R,
    ) -> Result<Option<SupervisorPostStartHookLaunchResult>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
        R: ProcessController + ?Sized,
    {
        let Some(job_id) = self.pending_post_hook_launches.front().copied() else {
            return Ok(None);
        };
        let launched_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let request = self.launch_request(job_id, launched_at_ns);

        let mut work = SupervisorWork::from_supervisor(self);
        work.pending_post_hook_launches.pop_front();

        let launch = launch_created_post_exec_hook_job_with_environment(
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
                return Ok(Some(SupervisorPostStartHookLaunchResult::PendingSetup(
                    pending,
                )));
            }
            Err(error) => {
                let Some(failure) = apply_post_start_hook_launch_failure(
                    &mut work,
                    controller,
                    job_id,
                    launched_at_ns,
                    error.clone(),
                    self.settings.phase2.max_parallel_starts,
                )?
                else {
                    return Err(SupervisorError::Launch(error));
                };
                work.commit(self);
                return Ok(Some(SupervisorPostStartHookLaunchResult::Failed(Box::new(
                    failure,
                ))));
            }
        };

        work.commit(self);

        Ok(Some(SupervisorPostStartHookLaunchResult::Launched(
            Box::new(SupervisorPostStartHookLaunchDispatch { launch }),
        )))
    }
}
