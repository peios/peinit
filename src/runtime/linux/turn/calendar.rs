#[cfg(feature = "peios-registry")]
#[cfg(feature = "peios-registry")]
use crate::runtime::collect_runtime_loop_console_messages;
use crate::runtime::{RuntimeCalendarTimerTurn, RuntimeEventSource, RuntimeShutdownLoopError};
use crate::supervisor::{Supervisor, SupervisorError};

#[cfg(feature = "peios-registry")]
use super::support::emit_runtime_loop_kmes_events;
#[cfg(feature = "peios-registry")]
use crate::registry::LcsTimerLastRunWriter;
use crate::runtime::linux::LinuxShutdownRuntime;
#[cfg(feature = "peios-registry")]
use crate::runtime::linux::calendar_timer::{
    LinuxCalendarTimerBootRegistration, LinuxCalendarTimerError,
};

impl LinuxShutdownRuntime {
    #[cfg(feature = "peios-registry")]
    pub(crate) fn register_calendar_timers(
        &mut self,
        supervisor: &mut Supervisor,
    ) -> Result<LinuxCalendarTimerBootRegistration, LinuxCalendarTimerError> {
        self.calendar_timers.register_boot_timers(
            supervisor,
            &mut self.clock,
            &mut self.registry,
            &mut LcsTimerLastRunWriter,
            &mut self.epoll,
        )
    }

    #[cfg(feature = "peios-registry")]
    pub(crate) fn emit_boot_calendar_timer_turns(
        &mut self,
        calendar_turns: &[(i32, RuntimeCalendarTimerTurn)],
    ) -> Result<(), RuntimeShutdownLoopError> {
        if calendar_turns.is_empty() {
            return Ok(());
        }
        emit_runtime_loop_kmes_events(
            &mut self.kmes_sink,
            &crate::runtime::RuntimeWorkPumpTurn::default(),
            &crate::supervisor::SupervisorOperationMaintenanceTurn::default(),
            &[],
            &crate::runtime::RuntimeWorkPumpTurn::default(),
            &crate::supervisor::SupervisorOperationMaintenanceTurn::default(),
            calendar_turns,
        )?;
        let mut console_messages = Vec::new();
        collect_runtime_loop_console_messages(
            &crate::runtime::RuntimeWorkPumpTurn::default(),
            &[],
            &crate::runtime::RuntimeWorkPumpTurn::default(),
            calendar_turns,
            &mut console_messages,
        );
        self.write_console_messages(console_messages);
        Ok(())
    }

    #[cfg(feature = "peios-registry")]
    pub(super) fn reconfigure_calendar_timers(
        &mut self,
        supervisor: &Supervisor,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeShutdownLoopError> {
        self.calendar_timers
            .reconfigure_timers(supervisor, &mut self.clock, &mut self.epoll)
            .map_err(|error| RuntimeShutdownLoopError::CalendarTimerReconfigure(error.to_string()))
    }

    pub(super) fn process_calendar_timer_sources(
        &mut self,
        supervisor: &mut Supervisor,
        sources: &[RuntimeEventSource],
    ) -> Result<Vec<(i32, RuntimeCalendarTimerTurn)>, RuntimeShutdownLoopError> {
        let mut turns = Vec::new();
        for source in sources {
            let RuntimeEventSource::CalendarTimer { fd } = *source else {
                continue;
            };
            #[cfg(feature = "peios-registry")]
            let result = self.calendar_timers.process_timer_event(
                supervisor,
                fd,
                &mut self.clock,
                &mut LcsTimerLastRunWriter,
            );
            #[cfg(not(feature = "peios-registry"))]
            let result = {
                let mut writer = crate::runtime::NoRuntimeRegistryClient;
                self.calendar_timers.process_timer_event(
                    supervisor,
                    fd,
                    &mut self.clock,
                    &mut writer,
                )
            };
            let turn = result.map_err(|error| RuntimeShutdownLoopError::Event {
                source: *source,
                error: crate::runtime::RuntimeShutdownEventTurnError::Supervisor(
                    SupervisorError::Timer(crate::boundary::BoundaryError::Timer(
                        error.to_string(),
                    )),
                ),
            })?;
            turns.push((fd, turn));
        }
        Ok(turns)
    }
}
