use crate::boundary::{Clock, ProcessLauncher, TokenProvider};
use crate::execution::launch::{
    LaunchCreatedJobResult, launch_created_reload_hook_job_with_environment,
};

use super::super::dispatch::{SupervisorControlLaunchDispatch, SupervisorControlLaunchResult};
use super::super::state::{Supervisor, SupervisorError};
use super::super::work::SupervisorWork;
use super::record_pending_process_setup;

impl Supervisor {
    pub fn launch_next_pending_control_job<T, P, C>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
    ) -> Result<Option<SupervisorControlLaunchResult>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
    {
        let Some(job_id) = self.pending_control_launches.front().copied() else {
            return Ok(None);
        };
        let launched_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let request = self.launch_request(job_id, launched_at_ns);

        let mut work = SupervisorWork::from_supervisor(self);
        work.pending_control_launches.pop_front();

        let launch = launch_created_reload_hook_job_with_environment(
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
                let pending = record_pending_process_setup(&mut work, setup)?;
                work.commit(self);
                return Ok(Some(SupervisorControlLaunchResult::PendingSetup(pending)));
            }
        };

        work.commit(self);

        Ok(Some(SupervisorControlLaunchResult::Launched(Box::new(
            SupervisorControlLaunchDispatch { launch },
        ))))
    }
}
