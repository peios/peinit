use crate::boundary::{ChildExitStatus, ProcessController, ShutdownFinalizer};
use crate::ids::JobId;
use crate::job::{JobExit, JobType};

use super::super::state::{Supervisor, SupervisorError};
use super::model::SupervisorChildReapDispatch;
use super::signal::signal_failure_cause;

impl Supervisor {
    pub(super) fn apply_runtime_reaped_job<P, F>(
        &mut self,
        job_id: JobId,
        job_type: JobType,
        status: ChildExitStatus,
        ended_at_ns: u64,
        controller: &mut P,
        finalizer: &mut F,
    ) -> Result<SupervisorChildReapDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        match job_type {
            JobType::ServiceMain | JobType::AdHoc => Ok(SupervisorChildReapDispatch::Runtime(
                Box::new(self.apply_service_main_reap(
                    job_id,
                    status,
                    ended_at_ns,
                    controller,
                    finalizer,
                )?),
            )),
            JobType::PreExecHook => Ok(SupervisorChildReapDispatch::PreStartHook(Box::new(
                match status {
                    ChildExitStatus::Exited { code } => {
                        self.complete_pre_start_hook_job(job_id, ended_at_ns, code, controller)?
                    }
                    ChildExitStatus::Signaled {
                        signal,
                        core_dumped,
                    } => self.fail_running_pre_start_hook_job(
                        job_id,
                        ended_at_ns,
                        Some(JobExit::Signal(signal)),
                        signal_failure_cause(signal, core_dumped),
                        controller,
                    )?,
                },
            ))),
            JobType::PostExecHook => Ok(SupervisorChildReapDispatch::PostStartHook(Box::new(
                match status {
                    ChildExitStatus::Exited { code } => {
                        self.complete_post_start_hook_job(job_id, ended_at_ns, code, controller)?
                    }
                    ChildExitStatus::Signaled {
                        signal,
                        core_dumped,
                    } => self.fail_running_post_start_hook_job(
                        job_id,
                        ended_at_ns,
                        Some(JobExit::Signal(signal)),
                        signal_failure_cause(signal, core_dumped),
                        controller,
                    )?,
                },
            ))),
            JobType::ReloadHook => Ok(SupervisorChildReapDispatch::ReloadCommand(Box::new(
                match status {
                    ChildExitStatus::Exited { code } => {
                        self.complete_reload_command_job(job_id, ended_at_ns, code, controller)?
                    }
                    ChildExitStatus::Signaled {
                        signal,
                        core_dumped,
                    } => self.fail_running_reload_command_job(
                        job_id,
                        ended_at_ns,
                        Some(JobExit::Signal(signal)),
                        signal_failure_cause(signal, core_dumped),
                        controller,
                    )?,
                },
            ))),
            JobType::HealthCheck => Ok(SupervisorChildReapDispatch::HealthCheck(Box::new(
                match status {
                    ChildExitStatus::Exited { code } => self
                        .complete_health_check_job_with_shutdown_finalizer(
                            job_id,
                            ended_at_ns,
                            code,
                            controller,
                            finalizer,
                        )?,
                    ChildExitStatus::Signaled {
                        signal,
                        core_dumped,
                    } => self.fail_running_health_check_job_with_shutdown_finalizer(
                        job_id,
                        ended_at_ns,
                        Some(JobExit::Signal(signal)),
                        signal_failure_cause(signal, core_dumped),
                        controller,
                        finalizer,
                    )?,
                },
            ))),
        }
    }

    fn apply_service_main_reap<P, F>(
        &mut self,
        job_id: JobId,
        status: ChildExitStatus,
        ended_at_ns: u64,
        controller: &mut P,
        finalizer: &mut F,
    ) -> Result<super::super::dispatch::SupervisorTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        match status {
            ChildExitStatus::Exited { code } => self.complete_job_with_runtime_context(
                job_id,
                ended_at_ns,
                code,
                controller,
                finalizer,
            ),
            ChildExitStatus::Signaled {
                signal,
                core_dumped,
            } => self.fail_running_job_with_runtime_context(
                job_id,
                ended_at_ns,
                Some(JobExit::Signal(signal)),
                signal_failure_cause(signal, core_dumped),
                controller,
                finalizer,
            ),
        }
    }
}
