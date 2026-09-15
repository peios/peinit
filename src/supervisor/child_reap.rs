mod model;
mod runtime;
mod signal;

pub use model::{SupervisorChildReapDispatch, SupervisorChildReapTurn};

use crate::boundary::{ChildExitStatus, ChildReap, ProcessController, ShutdownFinalizer};
use crate::job::{JobExit, JobType};

use super::state::{Supervisor, SupervisorError};
pub(in crate::supervisor) use signal::signal_failure_cause;

impl Supervisor {
    /// Is a launched-but-not-yet-started process using this pid?
    fn has_pending_setup_for_pid(&self, pid: u32) -> bool {
        self.pending_process_setups
            .values()
            .any(|setup| setup.process.pid == pid)
    }

    /// Exits held back by [`Self::apply_reaped_child`] whose job now exists.
    ///
    /// Drained by the runtime after a setup status is processed, and replayed
    /// through the ordinary reap path so that job-type routing, shutdown and
    /// submitted-job handling stay in one place.
    pub fn take_ready_deferred_reaps(&mut self) -> Vec<ChildReap> {
        let ready: Vec<u32> = self
            .reaped_before_setup
            .keys()
            .copied()
            .filter(|pid| self.jobs.active_job_by_pid(*pid).is_some())
            .collect();
        ready
            .into_iter()
            .filter_map(|pid| {
                self.reaped_before_setup
                    .remove(&pid)
                    .map(|status| ChildReap { pid, status })
            })
            .collect()
    }

    /// Apply a reaped child's exit to the job that owns its pid.
    ///
    /// The finalizer is for the reboot a Critical service earns by running
    /// out of restart budget here; given `None`, that reboot is left to
    /// [`Self::process_due_critical_budget_reboot`], which the runtime runs
    /// at the end of its turn once the turn's console output is written.
    pub fn apply_reaped_child<P>(
        &mut self,
        child: ChildReap,
        ended_at_ns: u64,
        controller: &mut P,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
    ) -> Result<SupervisorChildReapTurn, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let Some(job_id) = self.jobs.active_job_by_pid(child.pid) else {
            // No job carries this pid *yet*. A launched process is only findable
            // by pid once its setup status has been read and the job started,
            // and a short-lived child — a one-line ExecStartPre hook, say — can
            // be gone before peinit gets back to the setup pipe.
            //
            // Dropping the exit here is unrecoverable: the job is started
            // moments later against a pid that is already reaped, and no second
            // SIGCHLD is ever coming, so it stays Running for ever and its
            // service never leaves Starting. Hold the exit instead and replay it
            // once the job exists.
            if self.has_pending_setup_for_pid(child.pid) {
                self.reaped_before_setup.insert(child.pid, child.status);
                return Ok(SupervisorChildReapTurn::DeferredUntilSetup { child });
            }
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
