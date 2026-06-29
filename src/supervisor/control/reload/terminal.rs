use crate::boundary::ProcessController;
use crate::execution::control::complete_reload_command_job;
use crate::ids::JobId;
use crate::job::{JobEvent, JobEventDetail, JobExit, JobStore, JobStoreError};
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::SupervisorReloadCommandTerminalDispatch;
use crate::supervisor::health::apply_health_scheduling_after_transitions;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::watchdog::apply_watchdog_scheduling_after_transitions;
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn complete_reload_command_job<P>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit_code: i32,
        controller: &mut P,
    ) -> Result<SupervisorReloadCommandTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        self.apply_reload_command_terminal_job_event(job_id, controller, |jobs| {
            jobs.complete_job(job_id, ended_at_ns, exit_code)
        })
    }

    pub fn fail_running_reload_command_job<P>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
        controller: &mut P,
    ) -> Result<SupervisorReloadCommandTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let failure_cause = failure_cause.into();
        self.apply_reload_command_terminal_job_event(job_id, controller, |jobs| {
            jobs.fail_running_job(job_id, ended_at_ns, exit, failure_cause)
        })
    }

    fn apply_reload_command_terminal_job_event<F, P>(
        &mut self,
        job_id: JobId,
        controller: &mut P,
        event: F,
    ) -> Result<SupervisorReloadCommandTerminalDispatch, SupervisorError>
    where
        F: FnOnce(&mut JobStore) -> Result<crate::job::JobEvent, JobStoreError>,
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        let job = work
            .jobs
            .get(job_id)
            .cloned()
            .ok_or(JobStoreError::UnknownJob { id: job_id })
            .map_err(SupervisorError::JobStore)?;
        let cgroup_id = job.cgroup_id.clone();
        let service = job.service.clone();
        let job_event = event(&mut work.jobs).map_err(SupervisorError::JobStore)?;
        let terminal = complete_reload_command_job(
            &mut work.services,
            &mut work.operations,
            &mut work.control,
            job_event,
        )
        .map_err(SupervisorError::Control)?;
        let event_time = reload_terminal_event_time(&terminal.job_event)?;
        apply_health_scheduling_after_transitions(
            &mut work,
            std::slice::from_ref(&terminal.service_transition),
            event_time,
        );
        apply_watchdog_scheduling_after_transitions(
            &mut work,
            std::slice::from_ref(&terminal.service_transition),
            event_time,
        );
        apply_relationship_reactions_after_transitions(
            &mut work,
            std::slice::from_ref(&terminal.service_transition),
            event_time,
            self.settings().phase2.max_parallel_starts,
        )?;
        controller.kill_cgroup(&cgroup_id).map_err(|error| {
            SupervisorError::Control(crate::execution::control::ControlExecutionError::Boundary(
                error,
            ))
        })?;
        if let Some(service) = service.as_deref() {
            record_cgroup_cleanup(
                &mut work.cgroup_cleanup,
                service,
                &cgroup_id,
                CgroupCleanupKind::Hooks,
                event_time,
                self.settings().shutdown.post_kill_timeout_secs,
            );
        }
        work.commit(self);

        Ok(SupervisorReloadCommandTerminalDispatch { terminal })
    }
}

fn reload_terminal_event_time(job_event: &JobEvent) -> Result<u64, SupervisorError> {
    match job_event.detail {
        JobEventDetail::Ended { ended_at_ns, .. } => Ok(ended_at_ns),
        _ => Err(SupervisorError::Control(
            crate::execution::control::ControlExecutionError::NotTerminalJobEvent {
                job_id: job_event.job_id,
                state: job_event.state,
            },
        )),
    }
}
