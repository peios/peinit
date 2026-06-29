use std::io;

pub(crate) const TIMERFD_CREATE_CLOCK: i32 = libc::CLOCK_MONOTONIC;
pub(crate) const TIMERFD_REALTIME_CREATE_CLOCK: i32 = libc::CLOCK_REALTIME;
pub(crate) const TIMERFD_CREATE_FLAGS: i32 = libc::TFD_CLOEXEC | libc::TFD_NONBLOCK;
pub(crate) const TIMERFD_ABSOLUTE_FLAGS: i32 = libc::TFD_TIMER_ABSTIME;
pub(crate) const TIMERFD_REALTIME_ABSOLUTE_FLAGS: i32 =
    libc::TFD_TIMER_ABSTIME | libc::TFD_TIMER_CANCEL_ON_SET;

const NANOS_PER_SEC: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxTimerSpec {
    pub initial_sec: i64,
    pub initial_nsec: i64,
    pub interval_sec: i64,
    pub interval_nsec: i64,
}

impl LinuxTimerSpec {
    pub const fn disarmed() -> Self {
        Self {
            initial_sec: 0,
            initial_nsec: 0,
            interval_sec: 0,
            interval_nsec: 0,
        }
    }

    pub const fn absolute_oneshot(initial_sec: i64, initial_nsec: i64) -> Self {
        Self {
            initial_sec,
            initial_nsec,
            interval_sec: 0,
            interval_nsec: 0,
        }
    }
}

#[derive(Debug)]
pub enum LinuxTimerFdCreateError {
    Create {
        clock_id: i32,
        flags: i32,
        source: io::Error,
    },
    FdOutOfRange {
        returned_fd: i64,
    },
}

#[derive(Debug)]
pub enum LinuxTimerFdArmError {
    SetTime {
        fd: i32,
        flags: i32,
        spec: LinuxTimerSpec,
        source: io::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxTimerFdRead {
    Expired { expirations: u64 },
    Canceled,
    WouldBlock,
}

#[derive(Debug)]
pub enum LinuxTimerFdReadError {
    Read(io::Error),
    ShortRead { bytes: usize },
}

pub fn linux_timer_spec_from_deadline_ns(deadline_ns: u64) -> LinuxTimerSpec {
    let sec = deadline_ns / NANOS_PER_SEC;
    let nsec = deadline_ns % NANOS_PER_SEC;
    LinuxTimerSpec::absolute_oneshot(sec as i64, nsec as i64)
}
