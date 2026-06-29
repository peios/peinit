use crate::boundary::ProcessController;
use crate::execution::start::{
    StartExecutionContext, complete_post_start_hook_job as complete_post_start_hook_execution_job,
};
use crate::ids::JobId;
use crate::job::{JobEvent, JobExit, JobStore, JobStoreError};

use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::SupervisorPostStartHookTerminalDispatch;
use crate::supervisor::health::apply_health_scheduling_after_post_start;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::watchdog::apply_watchdog_scheduling_after_post_start;
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn complete_post_start_hook_job<P>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit_code: i32,
        controller: &mut P,
    ) -> Result<SupervisorPostStartHookTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        self.apply_post_start_hook_terminal_job_event(
            controller,
            |jobs| jobs.complete_job(job_id, ended_at_ns, exit_code),
            ended_at_ns,
        )
    }

    pub fn fail_running_post_start_hook_job<P>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
        controller: &mut P,
    ) -> Result<SupervisorPostStartHookTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let failure_cause = failure_cause.into();
        self.apply_post_start_hook_terminal_job_event(
            controller,
            |jobs| jobs.fail_running_job(job_id, ended_at_ns, exit, failure_cause),
            ended_at_ns,
        )
    }

    fn apply_post_start_hook_terminal_job_event<F, P>(
        &mut self,
        controller: &mut P,
        event: F,
        observed_at_ns: u64,
    ) -> Result<SupervisorPostStartHookTerminalDispatch, SupervisorError>
    where
        F: FnOnce(&mut JobStore) -> Result<JobEvent, JobStoreError>,
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);

        let job_event = event(&mut work.jobs).map_err(SupervisorError::JobStore)?;
        let terminal = complete_post_start_hook_in_work(&mut work, controller, job_event)?;
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
        schedule_after_final_post_start_hook(&mut work, &terminal, observed_at_ns);
        if let Some(cgroup_id) = &terminal.killed_cgroup_id
            && let Some(service) = terminal.job_event.service.as_deref()
        {
            record_cgroup_cleanup(
                &mut work.cgroup_cleanup,
                service,
                cgroup_id,
                CgroupCleanupKind::Hooks,
                observed_at_ns,
                self.settings.shutdown.post_kill_timeout_secs,
            );
        }

        work.commit(self);

        Ok(SupervisorPostStartHookTerminalDispatch {
            terminal,
            start_dispatches,
        })
    }
}

pub(super) fn complete_post_start_hook_in_work<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    job_event: JobEvent,
) -> Result<crate::execution::start::PostStartHookTerminalDispatch, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let terminal = complete_post_start_hook_execution_job(
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
        work.queue_created_post_hook_job(next_job_event);
    }
    Ok(terminal)
}

pub(super) fn schedule_after_final_post_start_hook(
    work: &mut SupervisorWork,
    terminal: &crate::execution::start::PostStartHookTerminalDispatch,
    observed_at_ns: u64,
) {
    if terminal.killed_cgroup_id.is_none() {
        return;
    }
    let Some(service) = terminal.job_event.service.as_deref() else {
        return;
    };
    schedule_after_post_start_service(work, service, observed_at_ns);
}

pub(super) fn schedule_after_post_start_service(
    work: &mut SupervisorWork,
    service: &str,
    observed_at_ns: u64,
) {
    apply_health_scheduling_after_post_start(work, service, observed_at_ns);
    apply_watchdog_scheduling_after_post_start(work, service, observed_at_ns);
}
