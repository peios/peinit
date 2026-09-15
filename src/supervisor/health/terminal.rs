use crate::boundary::{ProcessController, ShutdownFinalizer};
use crate::job::{JobEvent, JobExit, JobStore, JobStoreError};
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::critical_budget::CriticalRebootTrigger;
use crate::supervisor::dispatch::SupervisorHealthCheckTerminalDispatch;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::watchdog::apply_watchdog_scheduling_after_transitions;
use crate::supervisor::work::SupervisorWork;

use super::{HealthCheckError, apply_health_check_terminal_in_work, health_critical_reboot_due};
use crate::supervisor::health::terminate_service_after_health_escalation;

impl Supervisor {
    pub fn complete_health_check_job_with_shutdown_finalizer<P, F>(
        &mut self,
        job_id: crate::ids::JobId,
        ended_at_ns: u64,
        exit_code: i32,
        controller: &mut P,
        finalizer: &mut F,
    ) -> Result<SupervisorHealthCheckTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        self.apply_health_check_terminal_job_event(
            job_id,
            ended_at_ns,
            controller,
            Some(finalizer),
            |jobs| jobs.complete_job(job_id, ended_at_ns, exit_code),
        )
    }

    pub fn fail_running_health_check_job_with_shutdown_finalizer<P, F>(
        &mut self,
        job_id: crate::ids::JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
        controller: &mut P,
        finalizer: &mut F,
    ) -> Result<SupervisorHealthCheckTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        let failure_cause = failure_cause.into();
        self.apply_health_check_terminal_job_event(
            job_id,
            ended_at_ns,
            controller,
            Some(finalizer),
            |jobs| jobs.fail_running_job(job_id, ended_at_ns, exit, failure_cause),
        )
    }

    pub(in crate::supervisor) fn apply_health_check_terminal_job_event<F, P>(
        &mut self,
        job_id: crate::ids::JobId,
        observed_at_ns: u64,
        controller: &mut P,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
        event: F,
    ) -> Result<SupervisorHealthCheckTerminalDispatch, SupervisorError>
    where
        F: FnOnce(&mut JobStore) -> Result<JobEvent, JobStoreError>,
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        let job = work
            .jobs
            .get(job_id)
            .cloned()
            .ok_or(JobStoreError::UnknownJob { id: job_id })
            .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
        let cgroup_id = job.cgroup_id.clone();
        let service = job.service.clone();
        let job_event = event(&mut work.jobs)
            .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
        controller
            .kill_cgroup(&cgroup_id)
            .map_err(|error| SupervisorError::Health(HealthCheckError::Boundary(error)))?;
        let mut terminal = apply_health_check_terminal_in_work(
            &mut work,
            job_event,
            cgroup_id.clone(),
            observed_at_ns,
        )?;
        if let Some(service) = service.as_deref() {
            terminate_service_after_health_escalation(
                &mut work,
                &mut terminal,
                service,
                job.cgroup_generation,
                controller,
                observed_at_ns,
                self.settings.shutdown.post_kill_timeout_secs,
            )?;
        }
        apply_watchdog_scheduling_after_transitions(
            &mut work,
            &terminal.service_transitions,
            observed_at_ns,
        );
        let critical_reboot_due = health_critical_reboot_due(&work, &terminal);
        if !critical_reboot_due {
            apply_relationship_reactions_after_transitions(
                &mut work,
                &terminal.service_transitions,
                observed_at_ns,
                self.settings.phase2.max_parallel_starts,
            )?;
        }
        if let Some(service) = service.as_deref() {
            record_cgroup_cleanup(
                &mut work.cgroup_cleanup,
                service,
                &cgroup_id,
                CgroupCleanupKind::Health,
                observed_at_ns,
                self.settings.shutdown.post_kill_timeout_secs,
            );
        }

        work.commit(self);
        if critical_reboot_due {
            if let Some(finalizer) = finalizer {
                terminal.critical_reboot = Some(self.critical_reboot(finalizer, observed_at_ns)?);
            } else if let Some(service) = terminal.job_event.service.as_deref() {
                self.note_deferred_critical_reboot(
                    service,
                    CriticalRebootTrigger::HealthCheckFailure,
                    terminal.job_event.ended_at_ns,
                );
            }
        }

        Ok(terminal)
    }
}
