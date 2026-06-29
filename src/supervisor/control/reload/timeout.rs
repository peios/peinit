use crate::boundary::ProcessController;
use crate::execution::control::timeout_reload_command;
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::SupervisorReloadCommandTimeoutDispatch;
use crate::supervisor::health::apply_health_scheduling_after_transitions;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::watchdog::apply_watchdog_scheduling_after_transitions;
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn process_next_due_reload_command_timeout<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<SupervisorReloadCommandTimeoutDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let Some(deadline) = self
            .control
            .due_reload_command_deadlines(now_ns)
            .into_iter()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
        else {
            return Ok(None);
        };

        let mut work = SupervisorWork::from_supervisor(self);
        let timeout = timeout_reload_command(
            &mut work.services,
            &mut work.operations,
            &mut work.jobs,
            &mut work.control,
            controller,
            deadline,
            now_ns,
        )
        .map_err(SupervisorError::Control)?;
        apply_health_scheduling_after_transitions(
            &mut work,
            std::slice::from_ref(&timeout.service_transition),
            now_ns,
        );
        apply_watchdog_scheduling_after_transitions(
            &mut work,
            std::slice::from_ref(&timeout.service_transition),
            now_ns,
        );
        apply_relationship_reactions_after_transitions(
            &mut work,
            std::slice::from_ref(&timeout.service_transition),
            now_ns,
            self.settings().phase2.max_parallel_starts,
        )?;
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            timeout
                .job_event
                .service
                .as_deref()
                .unwrap_or(timeout.service_transition.event.service.as_str()),
            timeout.cgroup_id.clone(),
            CgroupCleanupKind::Hooks,
            now_ns,
            self.settings().shutdown.post_kill_timeout_secs,
        );
        work.commit(self);

        Ok(Some(SupervisorReloadCommandTimeoutDispatch { timeout }))
    }
}
