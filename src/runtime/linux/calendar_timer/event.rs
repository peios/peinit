use crate::boundary::{
    Clock, LinuxTimerFdRead, RealtimeClock, TimerLastRunWriteRequest, TimerLastRunWriter,
};
use crate::runtime::RuntimeCalendarTimerTurn;
use crate::supervisor::Supervisor;

use super::error::LinuxCalendarTimerError;
use super::random::jittered_deadline_ns;
use super::table::LinuxCalendarTimerTable;

#[cfg(test)]
mod tests;

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Where the search for the occurrence after a firing starts.
///
/// Anchored on the schedule, not on the clock: the deadline that just fired
/// was jittered forward, so by the time it fires the wall clock may already
/// be past the next un-jittered occurrence, and a search from "now" skips
/// that occurrence rather than delaying it — every run then lands on the
/// un-jittered time and only about one occurrence in `TimerJitter + 1` fires
/// at all (PEI-831). §9.4 says jitter only ever delays a firing.
///
/// The anchor is the occurrence that fired, except that anything whose whole
/// jitter window has already elapsed is left behind: §9.4 also says a missed
/// occurrence within one uptime fires once and is never replayed, so an
/// occurrence more than `TimerJitter` in the past — which no draw of the
/// jitter could still have delayed to now — is treated as missed rather than
/// caught up. With `TimerJitter=0` this is exactly "the next occurrence after
/// now".
fn rearm_anchor_ns(fired_scheduled_ns: u64, jitter_secs: u64, realtime_now_ns: u64) -> u64 {
    let jitter_ns = jitter_secs.saturating_mul(NANOS_PER_SEC);
    fired_scheduled_ns.max(realtime_now_ns.saturating_sub(jitter_ns))
}

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
        let (service, schedule, storage, anchor_ns) = {
            let entry = self
                .entries
                .get(&fd)
                .ok_or(LinuxCalendarTimerError::UnknownTimer { fd })?;
            (
                entry.service.clone(),
                entry.schedule.clone(),
                entry.storage,
                rearm_anchor_ns(entry.next_scheduled_ns, entry.jitter_secs, realtime_now_ns),
            )
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
        let next_scheduled_ns = self.rearm_after(fd, anchor_ns)?;
        Ok(RuntimeCalendarTimerTurn::Read {
            read,
            supervisor: Some(Box::new(supervisor_dispatch)),
            last_run_write: Some(last_run_write),
            next_scheduled_ns: Some(next_scheduled_ns),
        })
    }

    /// Arm the first occurrence strictly after `after_realtime_ns`.
    ///
    /// After a firing the caller passes [`rearm_anchor_ns`], not the clock;
    /// after a clock step that did not cross the occurrence it passes the new
    /// wall-clock time, which is the only sensible anchor there is.
    fn rearm_after(
        &mut self,
        fd: i32,
        after_realtime_ns: u64,
    ) -> Result<u64, LinuxCalendarTimerError> {
        let entry = self
            .entries
            .get_mut(&fd)
            .ok_or(LinuxCalendarTimerError::UnknownTimer { fd })?;
        let next_scheduled_ns = entry
            .calendar
            .next_after_ns(after_realtime_ns)
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
