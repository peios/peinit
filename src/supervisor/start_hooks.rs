use crate::execution::start::{
    StartExecutionContext, complete_pre_start_hook_job, timeout_pre_start_hook,
};
use crate::ids::JobId;
use crate::job::JobExit;

use super::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use super::dispatch::{
    SupervisorPreStartHookTerminalDispatch, SupervisorPreStartHookTimeoutDispatch,
};
use super::relationships::apply_relationship_reactions_after_transitions;
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

impl Supervisor {
    pub fn complete_pre_start_hook_job<P>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit_code: i32,
        controller: &mut P,
    ) -> Result<SupervisorPreStartHookTerminalDispatch, SupervisorError>
    where
        P: crate::boundary::ProcessController + ?Sized,
    {
        self.apply_pre_start_hook_terminal_job_event(
            controller,
            |jobs| jobs.complete_job(job_id, ended_at_ns, exit_code),
            ended_at_ns,
        )
    }

    pub fn fail_running_pre_start_hook_job<P>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
        controller: &mut P,
    ) -> Result<SupervisorPreStartHookTerminalDispatch, SupervisorError>
    where
        P: crate::boundary::ProcessController + ?Sized,
    {
        let failure_cause = failure_cause.into();
        self.apply_pre_start_hook_terminal_job_event(
            controller,
            |jobs| jobs.fail_running_job(job_id, ended_at_ns, exit, failure_cause),
            ended_at_ns,
        )
    }

    pub fn process_next_due_pre_start_hook_timeout<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<SupervisorPreStartHookTimeoutDispatch>, SupervisorError>
    where
        P: crate::boundary::ProcessController + ?Sized,
    {
        let Some(deadline) = self
            .start
            .due_pre_start_hook_deadlines(now_ns)
            .into_iter()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
        else {
            return Ok(None);
        };
        let service = deadline.service.clone();
        let service_cgroup_id = deadline.service_cgroup_id.clone();
        let hooks_cgroup_id = deadline.hooks_cgroup_id.clone();

        let mut work = SupervisorWork::from_supervisor(self);
        let timeout = timeout_pre_start_hook(
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
            service.clone(),
            hooks_cgroup_id,
            CgroupCleanupKind::Hooks,
            now_ns,
            self.settings.shutdown.post_kill_timeout_secs,
        );
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            service,
            service_cgroup_id,
            CgroupCleanupKind::ServiceTree,
            now_ns,
            self.settings.shutdown.post_kill_timeout_secs,
        );

        work.commit(self);

        Ok(Some(SupervisorPreStartHookTimeoutDispatch {
            timeout,
            start_dispatches,
        }))
    }

    fn apply_pre_start_hook_terminal_job_event<F, P>(
        &mut self,
        controller: &mut P,
        event: F,
        observed_at_ns: u64,
    ) -> Result<SupervisorPreStartHookTerminalDispatch, SupervisorError>
    where
        F: FnOnce(
            &mut crate::job::JobStore,
        ) -> Result<crate::job::JobEvent, crate::job::JobStoreError>,
        P: crate::boundary::ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);

        let job_event = event(&mut work.jobs).map_err(SupervisorError::JobStore)?;
        let terminal = complete_pre_start_hook_job(
            &mut StartExecutionContext {
                services: &mut work.services,
                operations: &mut work.operations,
                graph: &mut work.graph,
                jobs: &mut work.jobs,
                job_ids: &mut work.job_ids,
                start_store: &mut work.start,
                controller,
            },
            job_event,
        )
        .map_err(SupervisorError::Start)?;
        if let Some(next_job_event) = &terminal.next_job_event {
            work.queue_created_start_job(next_job_event);
        }
        let mut start_dispatches = apply_relationship_reactions_after_transitions(
            &mut work,
            &terminal.service_transitions,
            observed_at_ns,
            self.settings.phase2.max_parallel_starts,
        )?;
        start_dispatches.extend(work.release_after_graph_events(
            &terminal.graph_events,
            self.settings.phase2.max_parallel_starts,
            observed_at_ns,
        )?);
        if let Some(cgroup_id) = &terminal.killed_cgroup_id
            && let Some(service) = terminal.job_event.service.as_deref()
        {
            record_pre_start_hook_cleanup(
                &mut work,
                service,
                cgroup_id,
                observed_at_ns,
                self.settings.shutdown.post_kill_timeout_secs,
            );
        }

        work.commit(self);

        Ok(SupervisorPreStartHookTerminalDispatch {
            terminal,
            start_dispatches,
        })
    }
}

fn record_pre_start_hook_cleanup(
    work: &mut SupervisorWork,
    service: &str,
    cgroup_id: &str,
    observed_at_ns: u64,
    post_kill_timeout_secs: u64,
) {
    if cgroup_id.ends_with("/hooks") {
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            service,
            cgroup_id,
            CgroupCleanupKind::Hooks,
            observed_at_ns,
            post_kill_timeout_secs,
        );
    } else {
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            service,
            format!("{cgroup_id}/hooks"),
            CgroupCleanupKind::Hooks,
            observed_at_ns,
            post_kill_timeout_secs,
        );
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            service,
            cgroup_id,
            CgroupCleanupKind::ServiceTree,
            observed_at_ns,
            post_kill_timeout_secs,
        );
    }
}
