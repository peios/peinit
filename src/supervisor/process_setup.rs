use crate::boundary::{
    BoundaryError, LaunchedProcess, ProcessController, ProcessLaunchError, ProcessSetupStatus,
};
use crate::execution::launch::{LaunchCreatedJobDispatch, LaunchCreatedJobError};
use crate::job::{JobType, ProcessHandle};
use crate::supervisor::dispatch::{
    SupervisorControlLaunchDispatch, SupervisorHealthCheckLaunchFailureDispatch,
    SupervisorPendingProcessSetupDispatch, SupervisorPostStartHookLaunchDispatch,
    SupervisorProcessSetupDispatch, SupervisorStartHookLaunchDispatch,
};
use crate::supervisor::health::{
    apply_started_health_check_launch, fail_created_health_check_in_work,
    terminate_service_after_health_escalation,
};
use crate::supervisor::launch::{
    apply_pre_start_hook_launch_failure, apply_service_launch_failure, apply_started_service_launch,
};
use crate::supervisor::post_start_hooks::apply_post_start_hook_launch_failure;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::submitted::{apply_started_submitted_launch, apply_submitted_setup_failure};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn process_pending_process_setup_status<P>(
        &mut self,
        setup_status_fd: i32,
        status: ProcessSetupStatus,
        observed_at_ns: u64,
        controller: &mut P,
    ) -> Result<SupervisorProcessSetupDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        if matches!(status, ProcessSetupStatus::Pending) {
            return Ok(self
                .pending_setup_dispatch(setup_status_fd)
                .map(SupervisorProcessSetupDispatch::Pending)
                .unwrap_or(SupervisorProcessSetupDispatch::Stale { setup_status_fd }));
        }

        let mut work = SupervisorWork::from_supervisor(self);
        let Some(setup) = work.pending_process_setups.remove(&setup_status_fd) else {
            return Ok(SupervisorProcessSetupDispatch::Stale { setup_status_fd });
        };
        let job = work
            .jobs
            .get(setup.job_id)
            .cloned()
            .ok_or(crate::job::JobStoreError::UnknownJob { id: setup.job_id })
            .map_err(SupervisorError::JobStore)?;
        let job_type = job.job_type;

        let dispatch = match status {
            ProcessSetupStatus::ExecSucceeded => {
                let mut process = setup.process;
                process.setup_status_fd = None;
                let job_event = work
                    .jobs
                    .start_job_with_token_summary(
                        setup.job_id,
                        ProcessHandle {
                            pid: process.pid,
                            pidfd: process.pidfd,
                        },
                        setup.token_summary,
                        setup.launched_at_ns,
                    )
                    .map_err(SupervisorError::JobStore)?;
                let launch = LaunchCreatedJobDispatch {
                    job_id: setup.job_id,
                    process,
                    job_event,
                };
                complete_started_launch(
                    &mut work,
                    launch,
                    job_type,
                    setup.launched_at_ns,
                    self.settings.phase2.max_parallel_starts,
                )?
            }
            ProcessSetupStatus::PreExecFailed(error) => {
                let error = LaunchCreatedJobError::Boundary(BoundaryError::ProcessLaunch(
                    ProcessLaunchError::pre_exec(error, setup.process.cleanup_evidence.clone()),
                ));
                cleanup_pending_setup_process(&setup.process, &job.cgroup_id, controller)?;
                complete_failed_launch(
                    &mut work,
                    controller,
                    FailedLaunch {
                        job_id: setup.job_id,
                        job_type,
                        failed_at_ns: observed_at_ns,
                        error,
                        max_parallel_starts: self.settings.phase2.max_parallel_starts,
                        post_kill_timeout_secs: self.settings.shutdown.post_kill_timeout_secs,
                    },
                )?
            }
            ProcessSetupStatus::MalformedPreExec(message) => {
                let error = LaunchCreatedJobError::Boundary(BoundaryError::ProcessLaunch(
                    ProcessLaunchError::malformed_pre_exec(
                        message,
                        setup.process.cleanup_evidence.clone(),
                    ),
                ));
                cleanup_pending_setup_process(&setup.process, &job.cgroup_id, controller)?;
                complete_failed_launch(
                    &mut work,
                    controller,
                    FailedLaunch {
                        job_id: setup.job_id,
                        job_type,
                        failed_at_ns: observed_at_ns,
                        error,
                        max_parallel_starts: self.settings.phase2.max_parallel_starts,
                        post_kill_timeout_secs: self.settings.shutdown.post_kill_timeout_secs,
                    },
                )?
            }
            ProcessSetupStatus::Pending => unreachable!("pending setup status handled above"),
        };

        work.commit(self);
        Ok(dispatch)
    }

    fn pending_setup_dispatch(
        &self,
        setup_status_fd: i32,
    ) -> Option<SupervisorPendingProcessSetupDispatch> {
        let setup = self.pending_process_setups.get(&setup_status_fd)?;
        let job_type = self.jobs.get(setup.job_id)?.job_type;
        Some(SupervisorPendingProcessSetupDispatch {
            job_id: setup.job_id,
            job_type,
            setup_status_fd,
        })
    }
}

fn complete_started_launch(
    work: &mut SupervisorWork,
    launch: LaunchCreatedJobDispatch,
    job_type: JobType,
    launched_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<SupervisorProcessSetupDispatch, SupervisorError> {
    match job_type {
        JobType::ServiceMain => Ok(SupervisorProcessSetupDispatch::ServiceMainLaunched(
            Box::new(apply_started_service_launch(
                work,
                launch,
                launched_at_ns,
                max_parallel_starts,
            )?),
        )),
        JobType::PreExecHook => Ok(SupervisorProcessSetupDispatch::StartHookLaunched(
            SupervisorStartHookLaunchDispatch { launch },
        )),
        JobType::PostExecHook => Ok(SupervisorProcessSetupDispatch::PostHookLaunched(Box::new(
            SupervisorPostStartHookLaunchDispatch { launch },
        ))),
        JobType::ReloadHook => Ok(SupervisorProcessSetupDispatch::ControlLaunched(
            SupervisorControlLaunchDispatch { launch },
        )),
        JobType::HealthCheck => Ok(SupervisorProcessSetupDispatch::HealthCheckLaunched(
            apply_started_health_check_launch(work, launch, launched_at_ns)?,
        )),
        JobType::Submitted => Ok(SupervisorProcessSetupDispatch::SubmittedLaunched(
            apply_started_submitted_launch(work, launch)?,
        )),
    }
}

struct FailedLaunch {
    job_id: crate::ids::JobId,
    job_type: JobType,
    failed_at_ns: u64,
    error: LaunchCreatedJobError,
    max_parallel_starts: u32,
    post_kill_timeout_secs: u64,
}

fn complete_failed_launch<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    failed: FailedLaunch,
) -> Result<SupervisorProcessSetupDispatch, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    match failed.job_type {
        JobType::ServiceMain => apply_service_launch_failure(
            work,
            failed.job_id,
            failed.failed_at_ns,
            failed.error.clone(),
            failed.max_parallel_starts,
            failed.post_kill_timeout_secs,
        )?
        .map(|dispatch| SupervisorProcessSetupDispatch::ServiceMainFailed(Box::new(dispatch)))
        .ok_or(SupervisorError::Launch(failed.error)),
        JobType::PreExecHook => apply_pre_start_hook_launch_failure(
            work,
            controller,
            failed.job_id,
            failed.failed_at_ns,
            failed.error.clone(),
            failed.max_parallel_starts,
            failed.post_kill_timeout_secs,
        )?
        .map(|dispatch| SupervisorProcessSetupDispatch::StartHookFailed(Box::new(dispatch)))
        .ok_or(SupervisorError::Launch(failed.error)),
        JobType::PostExecHook => apply_post_start_hook_launch_failure(
            work,
            controller,
            failed.job_id,
            failed.failed_at_ns,
            failed.error.clone(),
            failed.max_parallel_starts,
        )?
        .map(|dispatch| SupervisorProcessSetupDispatch::PostHookFailed(Box::new(dispatch)))
        .ok_or(SupervisorError::Launch(failed.error)),
        JobType::HealthCheck => {
            let job = work
                .jobs
                .get(failed.job_id)
                .cloned()
                .ok_or(crate::job::JobStoreError::UnknownJob { id: failed.job_id })
                .map_err(SupervisorError::JobStore)?;
            let mut terminal = fail_created_health_check_in_work(
                work,
                failed.job_id,
                failed.failed_at_ns,
                format!("health check launch failed: {:?}", failed.error),
            )?;
            if let Some(service) = job.service.as_deref() {
                terminate_service_after_health_escalation(
                    work,
                    &mut terminal,
                    service,
                    job.cgroup_generation,
                    controller,
                    failed.failed_at_ns,
                    failed.post_kill_timeout_secs,
                )?;
            }
            Ok(SupervisorProcessSetupDispatch::HealthCheckFailed(Box::new(
                SupervisorHealthCheckLaunchFailureDispatch { terminal },
            )))
        }
        JobType::Submitted => Ok(SupervisorProcessSetupDispatch::SubmittedFailed(
            apply_submitted_setup_failure(
                work,
                controller,
                failed.job_id,
                failed.failed_at_ns,
                crate::submitted::SubmittedJobCause::PreExecFailure,
                format!("PreExecFailure: {:?}", failed.error),
                failed.post_kill_timeout_secs,
            )?,
        )),
        JobType::ReloadHook => Err(SupervisorError::Launch(failed.error)),
    }
}

fn cleanup_pending_setup_process<P>(
    process: &LaunchedProcess,
    cgroup_id: &str,
    controller: &mut P,
) -> Result<(), SupervisorError>
where
    P: ProcessController + ?Sized,
{
    controller
        .kill_cgroup(cgroup_id)
        .map_err(SupervisorError::ProcessControl)?;
    close_fd(process.pidfd);
    if let Some(fd) = process.stdout_fd {
        close_fd(fd);
    }
    if let Some(fd) = process.stderr_fd {
        close_fd(fd);
    }
    Ok(())
}

fn close_fd(fd: i32) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}
