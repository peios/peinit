use crate::boundary::LinuxTimerFd;
use crate::timer::calendar::CalendarSchedule;
use crate::timer::state::TimerLastRunStorage;

#[derive(Debug)]
pub(super) struct LinuxCalendarTimerEntry {
    pub(super) timer: LinuxTimerFd,
    pub(super) service: String,
    pub(super) schedule: String,
    pub(super) storage: TimerLastRunStorage,
    /// `TimerPersistent`: whether a firing records `LastTimerRun`. §9.3 says
    /// a non-persistent trigger ignores history entirely, and the write is
    /// a fork of PID 1 per firing (§14.4), so it is not made for one.
    pub(super) persistent: bool,
    pub(super) calendar: CalendarSchedule,
    pub(super) next_scheduled_ns: u64,
    pub(super) armed_deadline_ns: u64,
    pub(super) jitter_secs: u64,
}
