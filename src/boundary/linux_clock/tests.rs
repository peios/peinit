use std::collections::VecDeque;
use std::io;

use super::{LinuxClockError, LinuxClockSyscallApi, linux_monotonic_ns, linux_realtime_ns};

#[test]
fn monotonic_clock_converts_timespec_to_nanoseconds() {
    let mut syscalls = FakeClockSyscalls::with_results([Ok(libc::timespec {
        tv_sec: 12,
        tv_nsec: 345,
    })]);

    assert_eq!(
        linux_monotonic_ns(&mut syscalls).expect("monotonic ns"),
        12_000_000_345,
    );
    assert_eq!(syscalls.clock_ids, vec![libc::CLOCK_MONOTONIC]);
}

#[test]
fn realtime_clock_converts_timespec_to_nanoseconds() {
    let mut syscalls = FakeClockSyscalls::with_results([Ok(libc::timespec {
        tv_sec: 1_717_171_717,
        tv_nsec: 123_456_789,
    })]);

    assert_eq!(
        linux_realtime_ns(&mut syscalls).expect("realtime ns"),
        1_717_171_717_123_456_789,
    );
    assert_eq!(syscalls.clock_ids, vec![libc::CLOCK_REALTIME]);
}

#[test]
fn monotonic_clock_reports_syscall_failure() {
    let mut syscalls =
        FakeClockSyscalls::with_results([Err(io::Error::from(io::ErrorKind::PermissionDenied))]);

    assert!(matches!(
        linux_monotonic_ns(&mut syscalls).expect_err("clock error"),
        LinuxClockError::ClockGetTime { clock_id, .. } if clock_id == libc::CLOCK_MONOTONIC
    ));
}

#[test]
fn monotonic_clock_rejects_invalid_timespecs() {
    let mut negative = FakeClockSyscalls::with_results([Ok(libc::timespec {
        tv_sec: -1,
        tv_nsec: 0,
    })]);
    assert!(matches!(
        linux_monotonic_ns(&mut negative).expect_err("negative"),
        LinuxClockError::NegativeTime { .. },
    ));

    let mut invalid_nsec = FakeClockSyscalls::with_results([Ok(libc::timespec {
        tv_sec: 0,
        tv_nsec: 1_000_000_000,
    })]);
    assert!(matches!(
        linux_monotonic_ns(&mut invalid_nsec).expect_err("invalid nsec"),
        LinuxClockError::InvalidNanoseconds { .. },
    ));
}

#[derive(Debug)]
struct FakeClockSyscalls {
    results: VecDeque<io::Result<libc::timespec>>,
    clock_ids: Vec<i32>,
}

impl FakeClockSyscalls {
    fn with_results(results: impl IntoIterator<Item = io::Result<libc::timespec>>) -> Self {
        Self {
            results: results.into_iter().collect(),
            clock_ids: Vec::new(),
        }
    }
}

impl LinuxClockSyscallApi for FakeClockSyscalls {
    fn clock_gettime(&mut self, clock_id: i32) -> io::Result<libc::timespec> {
        self.clock_ids.push(clock_id);
        self.results.pop_front().expect("scripted clock result")
    }
}
