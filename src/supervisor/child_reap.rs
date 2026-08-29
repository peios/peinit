mod model;
mod runtime;
mod signal;

pub use model::{SupervisorChildReapDispatch, SupervisorChildReapTurn};

use crate::boundary::{ChildExitStatus, ChildReap, ProcessController, ShutdownFinalizer};
use crate::job::{JobExit, JobType};

use super::state::{Supervisor, SupervisorError};
pub(in crate::supervisor) use signal::signal_failure_cause;

impl Supervisor {
    pub fn apply_reaped_child<P, F>(
        &mut self,
        child: ChildReap,
        ended_at_ns: u64,
        controller: &mut P,
        finalizer: &mut F,
    ) -> Result<SupervisorChildReapTurn, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        let Some(job_id) = self.jobs.active_job_by_pid(child.pid) else {
            return Ok(SupervisorChildReapTurn::Untracked { child });
        };
        let job_type = self
            .jobs
            .get(job_id)
            .map(|job| job.job_type)
            .ok_or(crate::job::JobStoreError::UnknownJob { id: job_id })
            .map_err(SupervisorError::JobStore)?;

        let dispatch = if job_type == JobType::Submitted {
            let terminal =
                self.apply_submitted_reap(job_id, child.status, ended_at_ns, controller)?;
            if self.shutdown.is_some() {
                self.advance_shutdown_after_submitted_job(controller, ended_at_ns)?;
            }
            SupervisorChildReapDispatch::Submitted(Box::new(terminal))
        } else if self.shutdown.is_some() {
            SupervisorChildReapDispatch::Shutdown(Box::new(match child.status {
                ChildExitStatus::Exited { code } => {
                    self.complete_shutdown_job(job_id, ended_at_ns, code, controller)?
                }
                ChildExitStatus::Signaled {
                    signal,
                    core_dumped,
                } => self.fail_running_shutdown_job(
                    job_id,
                    ended_at_ns,
                    Some(JobExit::Signal(signal)),
                    signal_failure_cause(signal, core_dumped),
                    controller,
                )?,
            }))
        } else {
            self.apply_runtime_reaped_job(
                job_id,
                job_type,
                child.status,
                ended_at_ns,
                controller,
                finalizer,
            )?
        };

        Ok(SupervisorChildReapTurn::Tracked {
            child,
            job_id,
            dispatch,
        })
    }
}
