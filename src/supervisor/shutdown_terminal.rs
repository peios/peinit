mod exit;

use crate::boundary::ProcessController;
use crate::ids::JobId;
use crate::job::{JobEvent, JobExit};

use super::dispatch::SupervisorShutdownTerminalDispatch;
use super::shutdown_progress::{advance_shutdown_progress, ensure_shutdown};
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;
use exit::apply_shutdown_job_exit;

impl Supervisor {
    pub fn complete_shutdown_job<P>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit_code: i32,
        controller: &mut P,
    ) -> Result<SupervisorShutdownTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        ensure_shutdown(&work)?;
        let job_event = work
            .jobs
            .complete_job(job_id, ended_at_ns, exit_code)
            .map_err(SupervisorError::JobStore)?;
        finish_shutdown_terminal_job(self, work, job_event, ended_at_ns, controller)
    }

    pub fn fail_running_shutdown_job<P>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
        controller: &mut P,
    ) -> Result<SupervisorShutdownTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        ensure_shutdown(&work)?;
        let job_event = work
            .jobs
            .fail_running_job(job_id, ended_at_ns, exit, failure_cause)
            .map_err(SupervisorError::JobStore)?;
        finish_shutdown_terminal_job(self, work, job_event, ended_at_ns, controller)
    }
}

fn finish_shutdown_terminal_job<P>(
    supervisor: &mut Supervisor,
    mut work: SupervisorWork,
    job_event: JobEvent,
    ended_at_ns: u64,
    controller: &mut P,
) -> Result<SupervisorShutdownTerminalDispatch, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let service_transition =
        apply_shutdown_job_exit(&mut work, &job_event).map_err(SupervisorError::Shutdown)?;
    let next_wave = advance_shutdown_progress(&mut work, controller, ended_at_ns)
        .map_err(SupervisorError::Shutdown)?;
    let finalization = work
        .shutdown()
        .map_err(SupervisorError::Shutdown)?
        .finalization
        .clone();
    work.commit(supervisor);

    Ok(SupervisorShutdownTerminalDispatch {
        job_event,
        service_transition,
        next_wave,
        finalization,
    })
}
