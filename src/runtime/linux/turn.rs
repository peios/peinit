mod calendar;
mod control_config;
mod phase1;
mod support;

#[cfg(feature = "peios-registry")]
use self::support::{
    CalendarTimerMaintenanceStep, calendar_timer_maintenance_steps, reload_config_succeeded,
};
use self::support::{
    append_idle_closure_turn, emit_runtime_loop_kmes_events, flush_operation_waits_at,
    prepend_idle_closure_turn, process_due_operation_maintenance_at,
};
use crate::boundary::{Clock, ConsoleSink};
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
            let _ = self.console_sink.write_console(&message.text);
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
            };
            prepare_runtime_shutdown_loop_turn(supervisor, &mut event_sources, &mut context)?
        };
        let before_wait_ns = self
            .clock
            .monotonic_ns()
            .map_err(RuntimeShutdownLoopError::Clock)?;
        let maintenance_before_wait =
            process_due_operation_maintenance_at(supervisor, &mut self.controller, before_wait_ns)?;
        flush_operation_waits_at(supervisor, &mut self.control_connections, before_wait_ns)?;
        let idle_closed_before_wait = self.close_idle_control_connections(before_wait_ns);
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
            },
        )?;
        prepend_idle_closure_turn(&mut turn, idle_closed_before_wait);
        let after_sources_ns = self
            .clock
            .monotonic_ns()
            .map_err(RuntimeShutdownLoopError::Clock)?;
        let maintenance_after_sources = process_due_operation_maintenance_at(
            supervisor,
            &mut self.controller,
            after_sources_ns,
        )?;
        flush_operation_waits_at(supervisor, &mut self.control_connections, after_sources_ns)?;
        append_idle_closure_turn(
            &mut turn,
            self.close_idle_control_connections(after_sources_ns),
        );
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
        let mut console_messages = Vec::new();
        collect_runtime_loop_console_messages(
            &pre_work,
            &turn.turns,
            &turn.post_work,
            &calendar_turns,
            &mut console_messages,
        );
        self.write_console_messages(console_messages);
        turn.sources.extend(calendar_sources);
        turn.turns
            .extend(calendar_turns.into_iter().map(|(fd, turn)| {
                crate::runtime::RuntimeShutdownEventTurn::CalendarTimer { fd, turn }
            }));
        turn.pre_work = pre_work;
        Ok(turn)
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
