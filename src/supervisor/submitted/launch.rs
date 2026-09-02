use std::os::fd::{FromRawFd, OwnedFd};

use crate::boundary::{
    Clock, ProcessController, ProcessInheritedFd, ProcessLauncher, TokenProvider,
};
use crate::execution::launch::{
    LaunchCreatedJobError, LaunchCreatedJobResult, LaunchTokenSource, launch_created_submitted_job,
};
use crate::submitted::SubmittedJobCause;

use super::setup::apply_submitted_setup_failure;
use super::submit::close_fd;
use crate::supervisor::dispatch::{
    SupervisorSubmittedLaunchDispatch, SupervisorSubmittedLaunchResult,
};
use crate::supervisor::launch::record_pending_process_setup;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    /// Launch the next queued submitted job through the ordinary child path,
    /// with its prepared primary token and its attached descriptors.
    pub fn launch_next_pending_submitted_job<T, P, C, K>(
        &mut self,
        token_provider: &mut T,
        process_launcher: &mut P,
        clock: &mut C,
        controller: &mut K,
    ) -> Result<Option<SupervisorSubmittedLaunchResult>, SupervisorError>
    where
        T: TokenProvider + ?Sized,
        P: ProcessLauncher + ?Sized,
        C: Clock + ?Sized,
        K: ProcessController + ?Sized,
    {
        let Some(job_id) = crate::supervisor::pending_queue::next_live_front(
            &mut self.pending_submitted_launches,
            &self.jobs,
            &mut self.stale_launch_entries,
        ) else {
            return Ok(None);
        };
        let launched_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;

        let mut work = SupervisorWork::from_supervisor(self);
        work.pending_submitted_launches.pop_front();
        let global_environment = work.global_environment.clone();

        // Take what the entry holds for the launch. From here the entry owns
        // nothing the launch does not also own, so a failure closes them once.
        let (prepared_token_fd, inherited_fds) = {
            let entry = work
                .submitted
                .get_mut(job_id)
                .ok_or(SupervisorError::Submitted(
                    crate::submitted::SubmittedJobStoreError::UnknownJob { id: job_id },
                ))?;
            let prepared_token_fd = entry.prepared_token_fd.take();
            let inherited_fds = std::mem::take(&mut entry.attached_descriptors)
                .into_iter()
                .map(|(name, fd)| ProcessInheritedFd {
                    name,
                    fd: unsafe { OwnedFd::from_raw_fd(fd) },
                })
                .collect::<Vec<_>>();
            (prepared_token_fd, inherited_fds)
        };
        let post_kill_timeout_secs = self.settings.shutdown.post_kill_timeout_secs;
        let Some(prepared_token_fd) = prepared_token_fd else {
            let failure = apply_submitted_setup_failure(
                &mut work,
                controller,
                job_id,
                launched_at_ns,
                SubmittedJobCause::ParentSetupFailure,
                "ParentSetupFailure: no prepared token".to_string(),
                post_kill_timeout_secs,
            )?;
            work.commit(self);
            return Ok(Some(SupervisorSubmittedLaunchResult::Failed(failure)));
        };

        let request = self.launch_request_with_token(
            job_id,
            launched_at_ns,
            LaunchTokenSource::Prepared {
                token_fd: prepared_token_fd,
            },
        );
        let launch = launch_created_submitted_job(
            &mut work.jobs,
            token_provider,
            process_launcher,
            request,
            &global_environment,
            inherited_fds,
        );
        // The launch took its own copy of the token, or failed before it
        // could; either way the prepared descriptor is done with.
        close_fd(prepared_token_fd);

        let result = match launch {
            Ok(LaunchCreatedJobResult::PendingSetup(setup)) => {
                let pending = record_pending_process_setup(&mut work, setup)?;
                SupervisorSubmittedLaunchResult::PendingSetup(pending)
            }
            Ok(LaunchCreatedJobResult::Started(launch)) => {
                SupervisorSubmittedLaunchResult::Launched(apply_started_submitted_launch(
                    &mut work, *launch,
                )?)
            }
            Err(error) => {
                let failure = apply_submitted_setup_failure(
                    &mut work,
                    controller,
                    job_id,
                    launched_at_ns,
                    SubmittedJobCause::ParentSetupFailure,
                    launch_failure_cause(&error),
                    post_kill_timeout_secs,
                )?;
                SupervisorSubmittedLaunchResult::Failed(failure)
            }
        };
        work.commit(self);
        Ok(Some(result))
    }
}

/// The process is running: hand the output sink to the runtime.
pub(in crate::supervisor) fn apply_started_submitted_launch(
    work: &mut SupervisorWork,
    launch: crate::execution::launch::LaunchCreatedJobDispatch,
) -> Result<SupervisorSubmittedLaunchDispatch, SupervisorError> {
    let entry = work
        .submitted
        .get_mut(launch.job_id)
        .ok_or(SupervisorError::Submitted(
            crate::submitted::SubmittedJobStoreError::UnknownJob { id: launch.job_id },
        ))?;
    let output_sink_fd = entry.output_sink_fd.take();
    Ok(SupervisorSubmittedLaunchDispatch {
        launch,
        output_sink_fd,
    })
}

fn launch_failure_cause(error: &LaunchCreatedJobError) -> String {
    format!("ParentSetupFailure: {error:?}")
}
