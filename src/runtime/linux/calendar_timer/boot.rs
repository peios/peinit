use crate::boundary::{
    Clock, RealtimeClock, RegistryClient, TimerLastRunWriteRequest, TimerLastRunWriter,
};
use crate::runtime::{RuntimeCalendarTimerTurn, RuntimeEventRegistrar, RuntimeEventSource};
use crate::supervisor::Supervisor;
use crate::timer::boot::plan_timer_boot;

use super::error::LinuxCalendarTimerError;
use super::registration::{definitions_from_supervisor, register_calendar_timer};
use super::table::LinuxCalendarTimerTable;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinuxCalendarTimerBootRegistration {
    pub sources: Vec<RuntimeEventSource>,
    pub catch_up_turns: Vec<(i32, RuntimeCalendarTimerTurn)>,
}

impl LinuxCalendarTimerTable {
    pub(in crate::runtime::linux) fn register_boot_timers<C, R, W, E>(
        &mut self,
        supervisor: &mut Supervisor,
        clock: &mut C,
        registry: &mut R,
        writer: &mut W,
        registrar: &mut E,
    ) -> Result<LinuxCalendarTimerBootRegistration, LinuxCalendarTimerError>
    where
        C: Clock + RealtimeClock + ?Sized,
        R: RegistryClient + ?Sized,
        W: TimerLastRunWriter + ?Sized,
        E: RuntimeEventRegistrar + ?Sized,
    {
        let definitions = definitions_from_supervisor(supervisor);
        let realtime_now_ns = clock
            .realtime_ns()
            .map_err(LinuxCalendarTimerError::Clock)?;
        let plan = plan_timer_boot(&definitions, registry, realtime_now_ns)
            .map_err(LinuxCalendarTimerError::BootPlan)?;
        let mut sources = Vec::new();
        let mut catch_up_turns = Vec::new();
        for registration in plan.registrations {
            let (source, entry) = register_calendar_timer(registration.clone(), registrar)?;
            sources.push(source);
            self.entries.insert(entry.timer.as_raw_fd(), entry);
            if registration.missed_firing {
                let monotonic_now_ns = clock
                    .monotonic_ns()
                    .map_err(LinuxCalendarTimerError::Clock)?;
                let dispatch = supervisor
                    .handle_timer_firing(
                        &registration.service,
                        &registration.schedule,
                        monotonic_now_ns,
                    )
                    .map_err(LinuxCalendarTimerError::Supervisor)?;
                let last_run_write = writer.queue_timer_last_run_write(TimerLastRunWriteRequest {
                    service: registration.service.clone(),
                    schedule: registration.schedule.clone(),
                    storage: registration.storage,
                    timestamp_realtime_ns: realtime_now_ns,
                });
                let RuntimeEventSource::CalendarTimer { fd } = source else {
                    continue;
                };
                catch_up_turns.push((
                    fd,
                    RuntimeCalendarTimerTurn::Read {
                        read: crate::boundary::LinuxTimerFdRead::Expired { expirations: 1 },
                        supervisor: Some(Box::new(dispatch)),
                        last_run_write: Some(last_run_write),
                        next_scheduled_ns: Some(registration.next_scheduled_ns),
                    },
                ));
            }
        }
        Ok(LinuxCalendarTimerBootRegistration {
            sources,
            catch_up_turns,
        })
    }
}
