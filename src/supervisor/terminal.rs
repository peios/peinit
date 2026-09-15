use crate::boundary::{ProcessController, ShutdownFinalizer};
use crate::execution::job_terminal::apply_service_main_job_terminal;
use crate::execution::start::StartReadyContext;
use crate::ids::JobId;
use crate::job::JobExit;

use super::critical_budget::CriticalRebootTrigger;
use super::dispatch::SupervisorTerminalDispatch;
use super::health::apply_health_scheduling_after_transitions;
use super::relationships::apply_relationship_reactions_after_transitions;
use super::restart::begin_restart_start_after_stop;
use super::state::{Supervisor, SupervisorError};
use super::timer::start_pending_timer_runs_after_terminal;
use super::watchdog::apply_watchdog_scheduling_after_transitions;
use super::work::SupervisorWork;

mod cleanup;
mod critical;
mod event_time;

use cleanup::{
    NoReloadCleanupController, ReloadCleanupController, cancel_reload_after_main_exit,
    clear_fd_store_after_definition_discard, clear_fd_store_after_explicit_stop,
    remove_satisfied_readiness_deadlines, remove_satisfied_stop_deadlines,
};
use critical::critical_reboot_due;
use event_time::terminal_event_time;

impl Supervisor {
    pub fn complete_job(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit_code: i32,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError> {
        self.apply_terminal_job_event_no_controller(
            |jobs| jobs.complete_job(job_id, ended_at_ns, exit_code),
            None,
        )
    }

    pub fn complete_job_with_shutdown_finalizer<F>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit_code: i32,
        finalizer: &mut F,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError>
    where
        F: ShutdownFinalizer,
    {
        self.apply_terminal_job_event_no_controller(
            |jobs| jobs.complete_job(job_id, ended_at_ns, exit_code),
            Some(finalizer),
        )
    }

    pub fn complete_job_with_runtime_context<P, F>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit_code: i32,
        controller: &mut P,
        finalizer: &mut F,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        self.apply_terminal_job_event_with_controller(
            |jobs| jobs.complete_job(job_id, ended_at_ns, exit_code),
            Some(finalizer),
            controller,
        )
    }

    pub fn fail_running_job(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError> {
        let failure_cause = failure_cause.into();
        self.apply_terminal_job_event_no_controller(
            |jobs| jobs.fail_running_job(job_id, ended_at_ns, exit, failure_cause),
            None,
        )
    }

    pub fn fail_running_job_with_shutdown_finalizer<F>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
        finalizer: &mut F,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError>
    where
        F: ShutdownFinalizer,
    {
        let failure_cause = failure_cause.into();
        self.apply_terminal_job_event_no_controller(
            |jobs| jobs.fail_running_job(job_id, ended_at_ns, exit, failure_cause),
            Some(finalizer),
        )
    }

    pub fn fail_running_job_with_runtime_context<P, F>(
        &mut self,
        job_id: JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
        controller: &mut P,
        finalizer: &mut F,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        let failure_cause = failure_cause.into();
        self.apply_terminal_job_event_with_controller(
            |jobs| jobs.fail_running_job(job_id, ended_at_ns, exit, failure_cause),
            Some(finalizer),
            controller,
        )
    }

    fn apply_terminal_job_event_no_controller<F>(
        &mut self,
        event: F,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError>
    where
        F: FnOnce(
            &mut crate::job::JobStore,
        ) -> Result<crate::job::JobEvent, crate::job::JobStoreError>,
    {
        let mut controller = NoReloadCleanupController;
        self.apply_terminal_job_event(event, finalizer, &mut controller)
    }

    pub(super) fn apply_terminal_job_event_with_controller<F, P>(
        &mut self,
        event: F,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
        controller: &mut P,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError>
    where
        F: FnOnce(
            &mut crate::job::JobStore,
        ) -> Result<crate::job::JobEvent, crate::job::JobStoreError>,
        P: ProcessController + ?Sized,
    {
        self.apply_terminal_job_event(event, finalizer, controller)
    }

    fn apply_terminal_job_event<F, C>(
        &mut self,
        event: F,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
        cleanup_controller: &mut C,
    ) -> Result<SupervisorTerminalDispatch, SupervisorError>
    where
        F: FnOnce(
            &mut crate::job::JobStore,
        ) -> Result<crate::job::JobEvent, crate::job::JobStoreError>,
        C: ReloadCleanupController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);

        let job_event = event(&mut work.jobs).map_err(SupervisorError::JobStore)?;
        let mut terminal = apply_service_main_job_terminal(
            &mut StartReadyContext {
                services: &mut work.services,
                operations: &mut work.operations,
                graph: &mut work.graph,
                jobs: &mut work.jobs,
                job_ids: &mut work.job_ids,
                start_store: &mut work.start,
            },
            job_event,
        )
        .map_err(SupervisorError::JobTerminal)?;
        let terminal_event_time = terminal_event_time(&terminal)?;
        let cleanup_job_events = cancel_reload_after_main_exit(
            &mut work,
            &mut terminal,
            cleanup_controller,
            terminal_event_time,
            self.settings.shutdown.post_kill_timeout_secs,
        )?;
        if let Some(post_start_hook) = &terminal.post_start_hook {
            work.queue_created_post_hook_job(post_start_hook);
        }
        if terminal.post_start_hook.is_none() {
            apply_health_scheduling_after_transitions(
                &mut work,
                &terminal.service_transitions,
                terminal_event_time,
            );
            apply_watchdog_scheduling_after_transitions(
                &mut work,
                &terminal.service_transitions,
                terminal_event_time,
            );
        }
        remove_satisfied_readiness_deadlines(&mut work, &terminal);
        remove_satisfied_stop_deadlines(&mut work, &terminal);
        clear_fd_store_after_explicit_stop(&mut work, &terminal);
        clear_fd_store_after_definition_discard(&mut work, &terminal);
        let critical_reboot_due = critical_reboot_due(&work, &terminal);
        if critical_reboot_due {
            work.commit(self);
            let critical_reboot = if let Some(finalizer) = finalizer {
                Some(self.critical_reboot(finalizer, terminal_event_time)?)
            } else {
                if let Some(service) = terminal.job_event.service.as_deref() {
                    self.note_deferred_critical_reboot(
                        service,
                        CriticalRebootTrigger::ServiceMainTerminal,
                        terminal.job_event.ended_at_ns,
                    );
                }
                None
            };
            return Ok(SupervisorTerminalDispatch {
                terminal,
                cleanup_job_events,
                start_dispatches: Vec::new(),
                restart_start_dispatches: Vec::new(),
                critical_reboot,
            });
        }

        let mut restart_start_dispatches =
            begin_restart_start_after_stop(&mut work, &mut terminal, terminal_event_time)?;
        work.queue_restart_start_dispatches(&restart_start_dispatches);

        let mut start_dispatches = apply_relationship_reactions_after_transitions(
            &mut work,
            &terminal.service_transitions,
            terminal_event_time,
            self.settings.phase2.max_parallel_starts,
        )?;
        start_dispatches.extend(work.release_after_graph_events(
            &terminal.graph_events,
            self.settings.phase2.max_parallel_starts,
            terminal_event_time,
        )?);
        start_dispatches.extend(start_pending_timer_runs_after_terminal(
            &mut work,
            &terminal,
            terminal_event_time,
            self.settings.phase2.max_parallel_starts,
        )?);

        work.commit(self);

        Ok(SupervisorTerminalDispatch {
            terminal,
            cleanup_job_events,
            start_dispatches,
            restart_start_dispatches: std::mem::take(&mut restart_start_dispatches),
            critical_reboot: None,
        })
    }
}
