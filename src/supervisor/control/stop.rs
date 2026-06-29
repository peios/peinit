use crate::boundary::ProcessController;
use crate::execution::control::escalate_due_stop;

use super::super::cgroup_cleanup::{CgroupCleanupKind, parent_cgroup_path, record_cgroup_cleanup};
use super::super::dispatch::SupervisorStopEscalationDispatch;
use super::super::state::{Supervisor, SupervisorError};
use super::super::work::SupervisorWork;

impl Supervisor {
    pub fn process_next_due_stop_timeout<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<SupervisorStopEscalationDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let Some(deadline) = self
            .control
            .due_stop_timeouts(now_ns)
            .into_iter()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
        else {
            return Ok(None);
        };

        let mut work = SupervisorWork::from_supervisor(self);
        let escalation = escalate_due_stop(&mut work.control, controller, deadline, now_ns)
            .map_err(SupervisorError::Control)?;
        let root_cgroup_id = parent_cgroup_path(&escalation.cgroup_id)
            .unwrap_or_else(|| escalation.cgroup_id.clone());
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            escalation.service.clone(),
            escalation.cgroup_id.clone(),
            CgroupCleanupKind::StopMain {
                operation_id: escalation.operation_id,
                root_cgroup_id,
            },
            now_ns,
            self.settings().shutdown.post_kill_timeout_secs,
        );
        work.commit(self);

        Ok(Some(SupervisorStopEscalationDispatch { escalation }))
    }
}
