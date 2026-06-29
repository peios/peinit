use crate::boundary::ProcessController;
use crate::execution::start::{StartExecutionContext, timeout_post_start_hook};

use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::SupervisorPostStartHookTimeoutDispatch;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

use super::terminal::schedule_after_post_start_service;

impl Supervisor {
    pub fn process_next_due_post_start_hook_timeout<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<SupervisorPostStartHookTimeoutDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let Some(deadline) = self
            .start
            .due_post_start_hook_deadlines(now_ns)
            .into_iter()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
        else {
            return Ok(None);
        };
        let service = deadline.service.clone();
        let hooks_cgroup_id = deadline.hooks_cgroup_id.clone();
        let timed_out_job_id = deadline.job_id;

        let mut work = SupervisorWork::from_supervisor(self);
        let timeout = timeout_post_start_hook(
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
        work.pending_post_hook_launches
            .retain(|job_id| *job_id != timed_out_job_id);
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
        schedule_after_post_start_service(&mut work, &service, now_ns);
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            service,
            hooks_cgroup_id,
            CgroupCleanupKind::Hooks,
            now_ns,
            self.settings.shutdown.post_kill_timeout_secs,
        );

        work.commit(self);

        Ok(Some(SupervisorPostStartHookTimeoutDispatch {
            timeout,
            start_dispatches,
        }))
    }
}
