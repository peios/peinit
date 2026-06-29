use crate::boundary::ProcessController;
use crate::execution::start::{StartExecutionContext, timeout_readiness};

use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::SupervisorReadinessTimeoutDispatch;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn process_next_due_readiness_timeout<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<SupervisorReadinessTimeoutDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let Some(deadline) = self
            .start
            .due_readiness_deadlines(now_ns)
            .into_iter()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
        else {
            return Ok(None);
        };
        let service = deadline.service.clone();
        let service_cgroup_id = deadline.service_cgroup_id.clone();

        let mut work = SupervisorWork::from_supervisor(self);
        let timeout = timeout_readiness(
            &mut StartExecutionContext {
                services: &mut work.services,
                operations: &mut work.operations,
                graph: &mut work.graph,
                jobs: &mut work.jobs,
                job_ids: &mut work.job_ids,
                start_store: &mut work.start,
                controller,
            },
            deadline,
            now_ns,
        )
        .map_err(SupervisorError::Start)?;
        let mut start_dispatches = apply_relationship_reactions_after_transitions(
            &mut work,
            &timeout.service_transitions,
            now_ns,
            self.settings.phase2.max_parallel_starts,
        )?;
        start_dispatches.extend(work.release_after_graph_events(
            &timeout.graph_events,
            self.settings.phase2.max_parallel_starts,
            now_ns,
        )?);
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            service,
            service_cgroup_id,
            CgroupCleanupKind::ServiceTree,
            now_ns,
            self.settings.shutdown.post_kill_timeout_secs,
        );

        work.commit(self);

        Ok(Some(SupervisorReadinessTimeoutDispatch {
            timeout,
            start_dispatches,
        }))
    }
}
