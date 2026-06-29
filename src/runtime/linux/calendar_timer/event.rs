use crate::boundary::{
    Clock, LinuxTimerFdRead, RealtimeClock, TimerLastRunWriteRequest, TimerLastRunWriter,
};
use crate::runtime::RuntimeCalendarTimerTurn;
use crate::supervisor::Supervisor;

use super::error::LinuxCalendarTimerError;
use super::random::jittered_deadline_ns;
use super::table::LinuxCalendarTimerTable;

impl LinuxCalendarTimerTable {
    pub(in crate::runtime::linux) fn process_timer_event<C, W>(
        &mut self,
        supervisor: &mut Supervisor,
        fd: i32,
        clock: &mut C,
        writer: &mut W,
    ) -> Result<RuntimeCalendarTimerTurn, LinuxCalendarTimerError>
    where
        C: Clock + RealtimeClock + ?Sized,
        W: TimerLastRunWriter + ?Sized,
    {
        let read = self
            .entries
            .get_mut(&fd)
            .ok_or(LinuxCalendarTimerError::UnknownTimer { fd })?
            .timer
            .read_expirations()
            .map_err(LinuxCalendarTimerError::Read)?;

        match read {
            LinuxTimerFdRead::WouldBlock => Ok(RuntimeCalendarTimerTurn::Read {
                read,
                supervisor: None,
                last_run_write: None,
                next_scheduled_ns: None,
            }),
            LinuxTimerFdRead::Expired { .. } => {
                self.fire_and_rearm(supervisor, fd, read, clock, writer)
            }
            LinuxTimerFdRead::Canceled => {
                let realtime_now_ns = clock
                    .realtime_ns()
                    .map_err(LinuxCalendarTimerError::Clock)?;
                let due = self
                    .entries
                    .get(&fd)
                    .ok_or(LinuxCalendarTimerError::UnknownTimer { fd })?
                    .next_scheduled_ns
                    <= realtime_now_ns;
                if due {
                    self.fire_and_rearm_with_realtime(
                        supervisor,
                        fd,
                        read,
                        clock,
                        writer,
                        realtime_now_ns,
                    )
                } else {
                    let next_scheduled_ns = self.rearm_after(fd, realtime_now_ns)?;
                    Ok(RuntimeCalendarTimerTurn::Read {
                        read,
                        supervisor: None,
                        last_run_write: None,
                        next_scheduled_ns: Some(next_scheduled_ns),
                    })
                }
            }
        }
    }

    fn fire_and_rearm<C, W>(
        &mut self,
        supervisor: &mut Supervisor,
        fd: i32,
        read: LinuxTimerFdRead,
        clock: &mut C,
        writer: &mut W,
    ) -> Result<RuntimeCalendarTimerTurn, LinuxCalendarTimerError>
    where
        C: Clock + RealtimeClock + ?Sized,
        W: TimerLastRunWriter + ?Sized,
    {
        let realtime_now_ns = clock
            .realtime_ns()
            .map_err(LinuxCalendarTimerError::Clock)?;
        self.fire_and_rearm_with_realtime(supervisor, fd, read, clock, writer, realtime_now_ns)
    }

    fn fire_and_rearm_with_realtime<C, W>(
        &mut self,
        supervisor: &mut Supervisor,
        fd: i32,
        read: LinuxTimerFdRead,
        clock: &mut C,
        writer: &mut W,
        realtime_now_ns: u64,
    ) -> Result<RuntimeCalendarTimerTurn, LinuxCalendarTimerError>
    where
        C: Clock + RealtimeClock + ?Sized,
        W: TimerLastRunWriter + ?Sized,
    {
        let (service, schedule, storage) = {
            let entry = self
                .entries
                .get(&fd)
                .ok_or(LinuxCalendarTimerError::UnknownTimer { fd })?;
            (entry.service.clone(), entry.schedule.clone(), entry.storage)
        };
        let monotonic_now_ns = clock
            .monotonic_ns()
            .map_err(LinuxCalendarTimerError::Clock)?;
        let supervisor_dispatch = supervisor
            .handle_timer_firing(&service, &schedule, monotonic_now_ns)
            .map_err(LinuxCalendarTimerError::Supervisor)?;
        let last_run_write = writer.queue_timer_last_run_write(TimerLastRunWriteRequest {
            service: service.clone(),
            schedule: schedule.clone(),
            storage,
            timestamp_realtime_ns: realtime_now_ns,
        });
        let next_scheduled_ns = self.rearm_after(fd, realtime_now_ns)?;
        Ok(RuntimeCalendarTimerTurn::Read {
            read,
            supervisor: Some(Box::new(supervisor_dispatch)),
            last_run_write: Some(last_run_write),
            next_scheduled_ns: Some(next_scheduled_ns),
        })
    }

    fn rearm_after(
        &mut self,
        fd: i32,
        realtime_now_ns: u64,
    ) -> Result<u64, LinuxCalendarTimerError> {
        let entry = self
            .entries
            .get_mut(&fd)
            .ok_or(LinuxCalendarTimerError::UnknownTimer { fd })?;
        let next_scheduled_ns = entry
            .calendar
            .next_after_ns(realtime_now_ns)
            .map_err(LinuxCalendarTimerError::Next)?;
        let armed_deadline_ns = jittered_deadline_ns(next_scheduled_ns, entry.jitter_secs)?;
        entry
            .timer
            .arm_realtime_absolute_ns(armed_deadline_ns)
            .map_err(LinuxCalendarTimerError::Arm)?;
        entry.next_scheduled_ns = next_scheduled_ns;
        entry.armed_deadline_ns = armed_deadline_ns;
        Ok(next_scheduled_ns)
    }
}
