use crate::boundary::LinuxTimerFd;
use crate::timer::calendar::CalendarSchedule;
use crate::timer::state::TimerLastRunStorage;

#[derive(Debug)]
pub(super) struct LinuxCalendarTimerEntry {
    pub(super) timer: LinuxTimerFd,
    pub(super) service: String,
    pub(super) schedule: String,
    pub(super) storage: TimerLastRunStorage,
    pub(super) calendar: CalendarSchedule,
    pub(super) next_scheduled_ns: u64,
    pub(super) armed_deadline_ns: u64,
    pub(super) jitter_secs: u64,
}
