use std::io;

use super::{BoundaryError, Clock, RealtimeClock};

const MONOTONIC_CLOCK_ID: i32 = libc::CLOCK_MONOTONIC;
const REALTIME_CLOCK_ID: i32 = libc::CLOCK_REALTIME;
const NANOS_PER_SEC: u64 = 1_000_000_000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinuxMonotonicClock;

impl LinuxMonotonicClock {
    pub fn new() -> Self {
        Self
    }
}

impl Clock for LinuxMonotonicClock {
    fn monotonic_ns(&mut self) -> Result<u64, BoundaryError> {
        let mut syscalls = LinuxClockSyscalls;
        linux_monotonic_ns(&mut syscalls)
            .map_err(|error| BoundaryError::Clock(format!("{error:?}")))
    }
}

impl RealtimeClock for LinuxMonotonicClock {
    fn realtime_ns(&mut self) -> Result<u64, BoundaryError> {
        let mut syscalls = LinuxClockSyscalls;
        linux_realtime_ns(&mut syscalls).map_err(|error| BoundaryError::Clock(format!("{error:?}")))
    }
}

#[derive(Debug)]
pub enum LinuxClockError {
    ClockGetTime { clock_id: i32, source: io::Error },
    NegativeTime { sec: i64, nsec: i64 },
    InvalidNanoseconds { nsec: i64 },
    Overflow { sec: u64, nsec: u64 },
}

pub fn linux_monotonic_ns<S>(syscalls: &mut S) -> Result<u64, LinuxClockError>
where
    S: LinuxClockSyscallApi + ?Sized,
{
    linux_clock_ns(syscalls, MONOTONIC_CLOCK_ID)
}

pub fn linux_realtime_ns<S>(syscalls: &mut S) -> Result<u64, LinuxClockError>
where
    S: LinuxClockSyscallApi + ?Sized,
{
    linux_clock_ns(syscalls, REALTIME_CLOCK_ID)
}

fn linux_clock_ns<S>(syscalls: &mut S, clock_id: i32) -> Result<u64, LinuxClockError>
where
    S: LinuxClockSyscallApi + ?Sized,
{
    let spec = syscalls
        .clock_gettime(clock_id)
        .map_err(|source| LinuxClockError::ClockGetTime { clock_id, source })?;
    timespec_to_ns(spec)
}

pub trait LinuxClockSyscallApi {
    fn clock_gettime(&mut self, clock_id: i32) -> io::Result<libc::timespec>;
}

#[derive(Debug, Clone, Copy, Default)]
struct LinuxClockSyscalls;

impl LinuxClockSyscallApi for LinuxClockSyscalls {
    fn clock_gettime(&mut self, clock_id: i32) -> io::Result<libc::timespec> {
        let mut spec = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let rc = unsafe { libc::clock_gettime(clock_id, &mut spec) };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(spec)
        }
    }
}

fn timespec_to_ns(spec: libc::timespec) -> Result<u64, LinuxClockError> {
    let sec = spec.tv_sec;
    let nsec = spec.tv_nsec;
    if sec < 0 || nsec < 0 {
        return Err(LinuxClockError::NegativeTime { sec, nsec });
    }
    let nsec = u64::try_from(nsec).map_err(|_| LinuxClockError::NegativeTime { sec, nsec })?;
    if nsec >= NANOS_PER_SEC {
        return Err(LinuxClockError::InvalidNanoseconds { nsec: nsec as i64 });
    }
    let sec = u64::try_from(sec).map_err(|_| LinuxClockError::NegativeTime {
        sec,
        nsec: nsec as i64,
    })?;
    let sec_ns = sec
        .checked_mul(NANOS_PER_SEC)
        .ok_or(LinuxClockError::Overflow { sec, nsec })?;
    sec_ns
        .checked_add(nsec)
        .ok_or(LinuxClockError::Overflow { sec, nsec })
}

#[cfg(test)]
mod tests;
