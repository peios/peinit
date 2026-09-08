mod calendar;
mod control_config;
mod support;

#[cfg(feature = "peios-registry")]
use self::support::{
    CalendarTimerMaintenanceStep, calendar_timer_maintenance_steps, reload_config_succeeded,
};
use self::support::{
    append_idle_closure_turn, append_idle_jobs_closure_turn, emit_runtime_loop_kmes_events,
    flush_operation_waits_at, prepend_idle_closure_turn, prepend_idle_jobs_closure_turn,
    process_due_critical_budget_reboot_at, process_due_operation_maintenance_at,
};
use crate::boundary::{Clock, ConsoleSink, RealtimeClock};
use crate::runtime::{
    RuntimeEventRegistrationError, RuntimeEventSource, RuntimeEventWaiter,
    RuntimeShutdownEventSources, RuntimeShutdownLoopContext, RuntimeShutdownLoopError,
    RuntimeShutdownLoopTurn, collect_runtime_loop_console_messages,
    prepare_runtime_shutdown_loop_turn, process_runtime_shutdown_sources_with_registry,
};
use crate::supervisor::{
    Supervisor, SupervisorError, SupervisorLifecycleDeadlineTimerTurn,
    SupervisorShutdownDeadlineTimerTurn,
};

use super::LinuxShutdownRuntime;

impl LinuxShutdownRuntime {
    pub fn sync_shutdown_deadline_timer(
        &mut self,
        supervisor: &Supervisor,
    ) -> Result<SupervisorShutdownDeadlineTimerTurn, SupervisorError> {
        supervisor.sync_shutdown_deadline_timer(&mut self.deadline_timer)
    }

    pub fn sync_lifecycle_deadline_timer(
        &mut self,
        supervisor: &Supervisor,
    ) -> Result<SupervisorLifecycleDeadlineTimerTurn, SupervisorError> {
        supervisor.sync_lifecycle_deadline_timer(&mut self.lifecycle_timer)
    }

    /// Write what the quiet policy allows, dropping the rest.
    ///
    /// Dropped messages are gone, not deferred. That is a deliberate gap
    /// pending eventd: until there is somewhere to put them, `peios.quiet=0`
    /// is how an operator keeps the full narrative.
    fn write_console_messages(&mut self, messages: Vec<crate::runtime::console::ConsoleMessage>) {
        for message in messages {
            if !self.quiet_policy.allows(message.severity) {
                continue;
            }
            // Rendered here, at the one place that owns the device, rather
            // than at the dozens of producers: the tag column and its colours
            // are a property of the console, and a producer deep in the
            // supervisor has no way to know about either.
            let line = crate::console_style::render(message.tag, &message.text);
            let _ = self.console_sink.write_console(&line);
        }
    }

    pub fn run_turn(
        &mut self,
        supervisor: &mut Supervisor,
    ) -> Result<RuntimeShutdownLoopTurn, RuntimeShutdownLoopError> {
        self.sync_reloadable_config(supervisor);
        // Once per turn: terminal ownership only changes when a service does,
        // and a turn is the granularity at which that happens. Doing it here
        // also means both write sites below share one answer, so a message
        // cannot be judged against a different state than the one beside it.
        self.quiet_policy =
            crate::runtime::console::QuietPolicy::evaluate(self.quiet, supervisor.services());
        let pre_work = {
            let control_security = supervisor.control_security().clone();
            let control_limits = self.runtime_control_limits(supervisor);
            let mut event_sources = RuntimeShutdownEventSources {
                signal_source: &mut self.signal,
                child_reaper: &mut self.child_reaper,
                notify_source: &mut self.notify_socket,
                control_listener: &mut self.control_listener,
                control_connections: &mut self.control_connections,
                deadline_timer: &mut self.deadline_timer,
                lifecycle_timer: &mut self.lifecycle_timer,
                power_button_source: &mut self.power_buttons,
                filesystem_check_reader: &mut self.filesystem_check_reader,
                log_pipes: &mut self.log_pipes,
                jobs_channel: &mut self.jobs_channel,
            };
            let mut context = RuntimeShutdownLoopContext {
                clock: &mut self.clock,
                controller: &mut self.controller,
                finalizer: &mut self.finalizer,
                access_checker: &mut self.access_checker,
                registrar: &mut self.epoll,
                token_provider: &mut self.token_provider,
                process_launcher: &mut self.process_launcher,
                filesystem_check_launcher: &mut self.filesystem_check_launcher,
                boot_attempt_counter: &mut self.boot_attempt_counter,
                control_security: &control_security,
                max_events: self.config.max_events,
                control_limits,
                work_pump: self.config.work_pump.clone(),
                job_identity_provider: &mut self.job_identity_provider,
                jobs_limits: supervisor.jobs_limits(),
            };
            prepare_runtime_shutdown_loop_turn(supervisor, &mut event_sources, &mut context)?
        };
        let before_wait_ns = self
            .clock
            .monotonic_ns()
            .map_err(RuntimeShutdownLoopError::Clock)?;
        let before_wait_realtime_ns = self
            .clock
            .realtime_ns()
            .map_err(RuntimeShutdownLoopError::Clock)?;
        let maintenance_before_wait =
            process_due_operation_maintenance_at(supervisor, &mut self.controller, before_wait_ns)?;
        flush_operation_waits_at(
            supervisor,
            &mut self.control_connections,
            before_wait_ns,
            before_wait_realtime_ns,
        )?;
        let idle_closed_before_wait = self.close_idle_control_connections(before_wait_ns);
        let idle_jobs_closed_before_wait = self.close_idle_jobs_connections(before_wait_ns);
        let wait_timeout_ms = self.runtime_wait_timeout_ms(supervisor, before_wait_ns);
        let sources = self
            .epoll
            .wait_runtime_events_timeout(self.config.max_events, wait_timeout_ms)
            .map_err(RuntimeShutdownLoopError::Wait)?;
        let control_security = supervisor.control_security().clone();
        let control_limits = self.runtime_control_limits(supervisor);
        #[cfg(feature = "peios-registry")]
        let control_registry = Some(&mut self.registry);
        #[cfg(not(feature = "peios-registry"))]
        let control_registry = None::<&mut crate::runtime::NoRuntimeRegistryClient>;
        #[cfg(feature = "peios-registry")]
        let registry_watch =
            Some(&mut self.registry_watches as &mut dyn crate::boundary::RegistryWatchSource);
        #[cfg(not(feature = "peios-registry"))]
        let registry_watch = None;
        let (calendar_sources, other_sources): (Vec<_>, Vec<_>) = sources
            .into_iter()
            .partition(|source| matches!(source, RuntimeEventSource::CalendarTimer { .. }));
        let mut turn = process_runtime_shutdown_sources_with_registry(
            supervisor,
            other_sources,
            &mut RuntimeShutdownEventSources {
                signal_source: &mut self.signal,
                child_reaper: &mut self.child_reaper,
                notify_source: &mut self.notify_socket,
                control_listener: &mut self.control_listener,
                control_connections: &mut self.control_connections,
                deadline_timer: &mut self.deadline_timer,
                lifecycle_timer: &mut self.lifecycle_timer,
                power_button_source: &mut self.power_buttons,
                filesystem_check_reader: &mut self.filesystem_check_reader,
                log_pipes: &mut self.log_pipes,
                jobs_channel: &mut self.jobs_channel,
            },
            control_registry,
            registry_watch,
            RuntimeShutdownLoopContext {
                clock: &mut self.clock,
                controller: &mut self.controller,
                finalizer: &mut self.finalizer,
                access_checker: &mut self.access_checker,
                registrar: &mut self.epoll,
                token_provider: &mut self.token_provider,
                process_launcher: &mut self.process_launcher,
                filesystem_check_launcher: &mut self.filesystem_check_launcher,
                boot_attempt_counter: &mut self.boot_attempt_counter,
                control_security: &control_security,
                max_events: self.config.max_events,
                control_limits,
                work_pump: self.config.work_pump.clone(),
                job_identity_provider: &mut self.job_identity_provider,
                jobs_limits: supervisor.jobs_limits(),
            },
        )?;
        prepend_idle_closure_turn(&mut turn, idle_closed_before_wait);
        prepend_idle_jobs_closure_turn(&mut turn, idle_jobs_closed_before_wait);
        let after_sources_ns = self
            .clock
            .monotonic_ns()
            .map_err(RuntimeShutdownLoopError::Clock)?;
        let after_sources_realtime_ns = self
            .clock
            .realtime_ns()
            .map_err(RuntimeShutdownLoopError::Clock)?;
        // Replay any exit that was reaped before its job carried a pid. The
        // setup status that gives the job its pid may have been processed in
        // this very turn, and nothing else will deliver the exit again.
        let deferred_reaps = supervisor.take_ready_deferred_reaps();
        if !deferred_reaps.is_empty() {
            let mut child_reaps = Vec::with_capacity(deferred_reaps.len());
            for child in deferred_reaps {
                child_reaps.push(
                    supervisor
                        .apply_reaped_child(
                            child,
                            after_sources_ns,
                            &mut self.controller,
                            &mut self.finalizer,
                        )
                        .map_err(RuntimeShutdownLoopError::DeferredChildReap)?,
                );
            }
            turn.turns.push(
                crate::runtime::RuntimeShutdownEventTurn::DeferredChildReaps {
                    child_reaps,
                    ended_at_ns: after_sources_ns,
                },
            );
        }
        let maintenance_after_sources = process_due_operation_maintenance_at(
            supervisor,
            &mut self.controller,
            after_sources_ns,
        )?;
        // After the turn's events, so a service that exhausted its budget
        // anywhere in it is seen however it got there (PEI-341).
        let critical_budget_reboot = process_due_critical_budget_reboot_at(
            supervisor,
            &mut self.finalizer,
            after_sources_ns,
        )?;
        flush_operation_waits_at(
            supervisor,
            &mut self.control_connections,
            after_sources_ns,
            after_sources_realtime_ns,
        )?;
        append_idle_closure_turn(
            &mut turn,
            self.close_idle_control_connections(after_sources_ns),
        );
        let idle_jobs_closed_after = self.close_idle_jobs_connections(after_sources_ns);
        append_idle_jobs_closure_turn(&mut turn, idle_jobs_closed_after);
        self.sync_reloadable_config(supervisor);
        #[cfg(feature = "peios-registry")]
        let calendar_turns = {
            let mut calendar_turns = Vec::new();
            for step in calendar_timer_maintenance_steps(reload_config_succeeded(&turn.turns)) {
                match step {
                    CalendarTimerMaintenanceStep::ProcessReadyTimers => {
                        // Calendar fds returned by the current wait belong to
                        // the timer table that existed at wait time. Consume
                        // them before reload-config destroys and recreates it.
                        calendar_turns =
                            self.process_calendar_timer_sources(supervisor, &calendar_sources)?;
                    }
                    CalendarTimerMaintenanceStep::ReconfigureAfterReload => {
                        self.reconfigure_calendar_timers(supervisor)?;
                    }
                }
            }
            calendar_turns
        };
        #[cfg(not(feature = "peios-registry"))]
        let calendar_turns = self.process_calendar_timer_sources(supervisor, &calendar_sources)?;
        emit_runtime_loop_kmes_events(
            &mut self.kmes_sink,
            &pre_work,
            &maintenance_before_wait,
            &turn.turns,
            &turn.post_work,
            &maintenance_after_sources,
            &calendar_turns,
        )?;
        self.record_queued_timer_last_run_writes(&calendar_turns);
        let failed_last_run_writes = self.claim_timer_last_run_write_exits(&turn.turns);
        let mut console_messages = Vec::new();
        collect_runtime_loop_console_messages(
            &pre_work,
            &turn.turns,
            &turn.post_work,
            &calendar_turns,
            &mut console_messages,
        );
        for failed in &failed_last_run_writes {
            // The write is best-effort by design, so this is a warning rather
            // than a failure — but it has to be *said*. A persistent timer
            // whose timestamp never lands runs its catch-up on every boot, and
            // that is otherwise a mystery with no thread to pull (PEI-369).
            crate::runtime::console::push_error(
                &mut console_messages,
                format!(
                    "peinit warning: recording the last run of timer {} for service {} failed; \
                     it will run catch-up again after a reboot\n",
                    failed.schedule, failed.service,
                ),
            );
        }
        if let Some(reboot) = &critical_budget_reboot {
            crate::runtime::console::push_critical_budget_reboot_message(
                &mut console_messages,
                &reboot.service,
            );
        }
        self.write_console_messages(console_messages);
        turn.sources.extend(calendar_sources);
        turn.turns
            .extend(calendar_turns.into_iter().map(|(fd, turn)| {
                crate::runtime::RuntimeShutdownEventTurn::CalendarTimer { fd, turn }
            }));
        turn.pre_work = pre_work;
        Ok(turn)
    }

    /// Remember the children this turn forked, so their exits can be matched.
    fn record_queued_timer_last_run_writes(
        &mut self,
        calendar_turns: &[(i32, crate::runtime::RuntimeCalendarTimerTurn)],
    ) {
        for (fd, calendar_turn) in calendar_turns {
            let crate::runtime::RuntimeCalendarTimerTurn::Read {
                last_run_write: Some(Ok(crate::boundary::TimerLastRunWriteOutcome::Queued { pid })),
                ..
            } = calendar_turn
            else {
                continue;
            };
            let Some((service, schedule)) = self.calendar_timers.identity_for(*fd) else {
                continue;
            };
            self.timer_last_run_writes.record(*pid, service, schedule);
        }
    }

    /// Match this turn's untracked child reaps against outstanding writes.
    fn claim_timer_last_run_write_exits(
        &mut self,
        turns: &[crate::runtime::RuntimeShutdownEventTurn],
    ) -> Vec<super::timer_last_run::FailedTimerLastRunWrite> {
        let mut failed = Vec::new();
        for event_turn in turns {
            let crate::runtime::RuntimeShutdownEventTurn::Pid1Signal { child_reaps, .. } =
                event_turn
            else {
                continue;
            };
            for reap in child_reaps {
                let crate::supervisor::SupervisorChildReapTurn::Untracked { child } = reap else {
                    continue;
                };
                if let Some(write) = self.timer_last_run_writes.claim(child.pid, child.status) {
                    failed.push(write);
                }
            }
        }
        failed
    }

    pub fn run_forever(
        &mut self,
        supervisor: &mut Supervisor,
    ) -> Result<(), RuntimeShutdownLoopError> {
        loop {
            self.run_turn(supervisor)?;
        }
    }

    pub fn register_retained_service_launches(
        &mut self,
        supervisor: &mut Supervisor,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventRegistrationError> {
        let registrations = self
            .log_pipes
            .register_retained_launches(supervisor.retained_service_launches(), &mut self.epoll)?;
        supervisor.drain_retained_service_launches();
        Ok(registrations)
    }
}

#[cfg(test)]
mod tests;
