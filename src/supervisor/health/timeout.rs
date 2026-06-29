use crate::boundary::{ProcessController, ShutdownFinalizer};
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::SupervisorHealthCheckTimeoutDispatch;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::watchdog::apply_watchdog_scheduling_after_transitions;
use crate::supervisor::work::SupervisorWork;

use super::{
    HealthCheckError, fail_timed_out_health_check_in_work, health_critical_reboot_due,
    terminate_service_after_health_escalation,
};

impl Supervisor {
    pub fn process_due_health_check_timeouts<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Vec<SupervisorHealthCheckTimeoutDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        self.process_due_health_check_timeouts_with_finalizer(controller, None, now_ns)
    }

    pub fn process_due_health_check_timeouts_with_finalizer<P>(
        &mut self,
        controller: &mut P,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
        now_ns: u64,
    ) -> Result<Vec<SupervisorHealthCheckTimeoutDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let due = self.health.due_timeout_deadlines(now_ns);
        let mut work = SupervisorWork::from_supervisor(self);
        let mut dispatches = Vec::with_capacity(due.len());
        let mut critical_reboot_index = None;

        for deadline in due {
            let service = deadline.service.clone();
            let health_cgroup_id = deadline.health_cgroup_id.clone();
            controller
                .kill_cgroup(&deadline.health_cgroup_id)
                .map_err(|error| SupervisorError::Health(HealthCheckError::Boundary(error)))?;
            let (terminal, _) =
                fail_timed_out_health_check_in_work(&mut work, deadline.job_id, now_ns)?;
            let mut terminal = terminal;
            terminate_service_after_health_escalation(
                &mut work,
                &mut terminal,
                &service,
                deadline.cgroup_generation,
                controller,
                now_ns,
                self.settings.shutdown.post_kill_timeout_secs,
            )?;
            apply_watchdog_scheduling_after_transitions(
                &mut work,
                &terminal.service_transitions,
                now_ns,
            );
            let critical_reboot_due = health_critical_reboot_due(&work, &terminal);
            if critical_reboot_index.is_none() && critical_reboot_due {
                critical_reboot_index = Some(dispatches.len());
            }
            if !critical_reboot_due {
                apply_relationship_reactions_after_transitions(
                    &mut work,
                    &terminal.service_transitions,
                    now_ns,
                    self.settings.phase2.max_parallel_starts,
                )?;
            }
            record_cgroup_cleanup(
                &mut work.cgroup_cleanup,
                service,
                health_cgroup_id,
                CgroupCleanupKind::Health,
                now_ns,
                self.settings.shutdown.post_kill_timeout_secs,
            );
            dispatches.push(SupervisorHealthCheckTimeoutDispatch {
                terminal,
                timed_out_at_ns: now_ns,
            });
        }

        work.commit(self);
        if let Some(index) = critical_reboot_index
            && let Some(finalizer) = finalizer
        {
            dispatches[index].terminal.critical_reboot =
                Some(self.critical_reboot(finalizer, now_ns)?);
        }
        Ok(dispatches)
    }
}
