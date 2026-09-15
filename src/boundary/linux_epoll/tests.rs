use std::collections::VecDeque;
use std::io;

use crate::boundary::linux_epoll::{
    EPOLL_CREATE_FLAGS, LinuxEpollCreateError, LinuxEpollEvent, LinuxEpollRegisterError,
    LinuxEpollSyscallApi, LinuxEpollUnregisterError, LinuxEpollWaitError, create_linux_epoll,
    register_linux_epoll_read, unregister_linux_epoll, wait_linux_epoll,
};

#[test]
fn create_linux_epoll_uses_close_on_exec_flag() {
    let mut syscalls = FakeEpollSyscalls::default();

    let fd = create_linux_epoll(&mut syscalls).expect("create epoll");

    assert_eq!(fd, 23);
    assert_eq!(
        syscalls.calls,
        vec![FakeEpollCall::Create {
            flags: EPOLL_CREATE_FLAGS,
        }],
    );
}

#[test]
fn create_linux_epoll_reports_create_failure_and_fd_overflow() {
    let mut syscalls = FakeEpollSyscalls {
        create_error: Some(io::ErrorKind::PermissionDenied),
        ..FakeEpollSyscalls::default()
    };
    assert!(matches!(
        create_linux_epoll(&mut syscalls).expect_err("create failure"),
        LinuxEpollCreateError::Create {
            flags: EPOLL_CREATE_FLAGS,
            ..
        },
    ));

    let mut syscalls = FakeEpollSyscalls {
        create_fd: i64::from(i32::MAX) + 1,
        ..FakeEpollSyscalls::default()
    };
    assert!(matches!(
        create_linux_epoll(&mut syscalls).expect_err("fd overflow"),
        LinuxEpollCreateError::FdOutOfRange { returned_fd }
            if returned_fd == i64::from(i32::MAX) + 1
    ));
}

#[test]
fn register_linux_epoll_read_adds_read_interest_with_stable_token() {
    let mut syscalls = FakeEpollSyscalls::default();

    register_linux_epoll_read(&mut syscalls, 23, 41, 9001).expect("register");

    assert_eq!(
        syscalls.calls,
        vec![FakeEpollCall::Ctl {
            epoll_fd: 23,
            op: libc::EPOLL_CTL_ADD,
            fd: 41,
            event: Some(LinuxEpollEvent::read(9001)),
        }],
    );
}

#[test]
fn register_linux_epoll_read_reports_ctl_failure() {
    let mut syscalls = FakeEpollSyscalls {
        ctl_error: Some(io::ErrorKind::InvalidInput),
        ..FakeEpollSyscalls::default()
    };

    let err = register_linux_epoll_read(&mut syscalls, 23, 41, 9001).expect_err("ctl failure");

    assert!(matches!(
        err,
        LinuxEpollRegisterError::Control {
            epoll_fd: 23,
            op: libc::EPOLL_CTL_ADD,
            fd: 41,
            event: LinuxEpollEvent { token: 9001, .. },
            ..
        },
    ));
}

#[test]
fn unregister_linux_epoll_deletes_source_without_event_payload() {
    let mut syscalls = FakeEpollSyscalls::default();

    unregister_linux_epoll(&mut syscalls, 23, 41).expect("unregister");

    assert_eq!(
        syscalls.calls,
        vec![FakeEpollCall::Ctl {
            epoll_fd: 23,
            op: libc::EPOLL_CTL_DEL,
            fd: 41,
            event: None,
        }],
    );
}

#[test]
fn unregister_linux_epoll_reports_ctl_failure() {
    let mut syscalls = FakeEpollSyscalls {
        ctl_error: Some(io::ErrorKind::InvalidInput),
        ..FakeEpollSyscalls::default()
    };

    let err = unregister_linux_epoll(&mut syscalls, 23, 41).expect_err("ctl failure");

    assert!(matches!(
        err,
        LinuxEpollUnregisterError::Control {
            epoll_fd: 23,
            op: libc::EPOLL_CTL_DEL,
            fd: 41,
            ..
        },
    ));
}

#[test]
fn wait_linux_epoll_returns_ready_events_and_rejects_empty_buffers() {
    let ready = vec![
        LinuxEpollEvent::read(1),
        LinuxEpollEvent {
            events: libc::EPOLLERR as u32,
            token: 2,
        },
    ];
    let mut syscalls = FakeEpollSyscalls {
        wait_events: ready.clone(),
        // A timed wait notes when it started, in case it has to be retried.
        monotonic_ns: VecDeque::from([1_000_000_000]),
        ..FakeEpollSyscalls::default()
    };

    let events = wait_linux_epoll(&mut syscalls, 23, 16, 250).expect("wait");

    assert_eq!(events, ready);
    assert_eq!(
        syscalls.calls,
        vec![FakeEpollCall::Wait {
            epoll_fd: 23,
            max_events: 16,
            timeout_ms: 250,
        }],
    );
    assert_eq!(syscalls.clock_reads, 1);
    assert!(matches!(
        wait_linux_epoll(&mut syscalls, 23, 0, 250).expect_err("empty buffer"),
        LinuxEpollWaitError::InvalidMaxEvents,
    ));
}

#[test]
fn wait_linux_epoll_reports_wait_failure() {
    let mut syscalls = FakeEpollSyscalls {
        wait_errors: VecDeque::from([io::ErrorKind::InvalidInput]),
        ..FakeEpollSyscalls::default()
    };

    let err = wait_linux_epoll(&mut syscalls, 23, 16, -1).expect_err("wait failure");

    assert!(matches!(
        err,
        LinuxEpollWaitError::Wait {
            epoll_fd: 23,
            timeout_ms: -1,
            ..
        },
    ));
    assert_eq!(syscalls.calls.len(), 1, "a real failure is not retried");
}

// PEI-1085. A ptrace stop interrupts a timed epoll_wait with EINTR even when
// no handler ran; the wait is where PID 1 lives, so before this a debugger
// attaching to peinit ended its runtime loop. The retry keeps the original
// deadline: the clock ran on while peinit was stopped.
#[test]
fn wait_linux_epoll_retries_an_interrupted_wait_with_the_remaining_timeout() {
    let ready = vec![LinuxEpollEvent::read(7)];
    let mut syscalls = FakeEpollSyscalls {
        wait_events: ready.clone(),
        wait_errors: VecDeque::from([io::ErrorKind::Interrupted, io::ErrorKind::Interrupted]),
        // Before the wait, then after each interruption: 100 ms and 249.5 ms in.
        monotonic_ns: VecDeque::from([1_000_000_000, 1_100_000_000, 1_249_500_000]),
        ..FakeEpollSyscalls::default()
    };

    let events = wait_linux_epoll(&mut syscalls, 23, 16, 250).expect("wait");

    assert_eq!(events, ready);
    assert_eq!(
        syscalls.calls,
        vec![
            FakeEpollCall::Wait {
                epoll_fd: 23,
                max_events: 16,
                timeout_ms: 250,
            },
            FakeEpollCall::Wait {
                epoll_fd: 23,
                max_events: 16,
                timeout_ms: 150,
            },
            // Rounded up, never down: a retry must not return early.
            FakeEpollCall::Wait {
                epoll_fd: 23,
                max_events: 16,
                timeout_ms: 1,
            },
        ],
    );
}

#[test]
fn wait_linux_epoll_retries_an_interrupted_wait_past_its_deadline_without_blocking() {
    let mut syscalls = FakeEpollSyscalls {
        wait_errors: VecDeque::from([io::ErrorKind::Interrupted]),
        monotonic_ns: VecDeque::from([1_000_000_000, 5_000_000_000]),
        ..FakeEpollSyscalls::default()
    };

    let events = wait_linux_epoll(&mut syscalls, 23, 16, 250).expect("wait");

    assert!(events.is_empty());
    assert!(matches!(
        syscalls.calls.as_slice(),
        [
            FakeEpollCall::Wait {
                timeout_ms: 250,
                ..
            },
            FakeEpollCall::Wait { timeout_ms: 0, .. },
        ],
    ));
}

#[test]
fn wait_linux_epoll_retries_an_interrupted_unbounded_wait_without_reading_the_clock() {
    let mut syscalls = FakeEpollSyscalls {
        wait_errors: VecDeque::from([io::ErrorKind::Interrupted]),
        ..FakeEpollSyscalls::default()
    };

    wait_linux_epoll(&mut syscalls, 23, 16, -1).expect("wait");

    assert!(matches!(
        syscalls.calls.as_slice(),
        [
            FakeEpollCall::Wait { timeout_ms: -1, .. },
            FakeEpollCall::Wait { timeout_ms: -1, .. },
        ],
    ));
    assert_eq!(syscalls.clock_reads, 0);
}

#[test]
fn wait_linux_epoll_reports_a_clock_failure_while_retrying() {
    let mut syscalls = FakeEpollSyscalls {
        wait_errors: VecDeque::from([io::ErrorKind::Interrupted]),
        monotonic_ns: VecDeque::from([1_000_000_000]),
        ..FakeEpollSyscalls::default()
    };

    let err = wait_linux_epoll(&mut syscalls, 23, 16, 250).expect_err("clock failure");

    assert!(matches!(err, LinuxEpollWaitError::Clock { .. }));
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FakeEpollCall {
    Create {
        flags: i32,
    },
    Ctl {
        epoll_fd: i32,
        op: i32,
        fd: i32,
        event: Option<LinuxEpollEvent>,
    },
    Wait {
        epoll_fd: i32,
        max_events: usize,
        timeout_ms: i32,
    },
}

#[derive(Debug)]
struct FakeEpollSyscalls {
    create_fd: i64,
    create_error: Option<io::ErrorKind>,
    ctl_error: Option<io::ErrorKind>,
    wait_events: Vec<LinuxEpollEvent>,
    /// Consumed one per wait; once empty, waits succeed.
    wait_errors: VecDeque<io::ErrorKind>,
    /// Consumed one per clock read; once empty, the clock fails.
    monotonic_ns: VecDeque<u64>,
    clock_reads: usize,
    calls: Vec<FakeEpollCall>,
}

impl Default for FakeEpollSyscalls {
    fn default() -> Self {
        Self {
            create_fd: 23,
            create_error: None,
            ctl_error: None,
            wait_events: Vec::new(),
            wait_errors: VecDeque::new(),
            monotonic_ns: VecDeque::new(),
            clock_reads: 0,
            calls: Vec::new(),
        }
    }
}

impl LinuxEpollSyscallApi for FakeEpollSyscalls {
    fn monotonic_ns(&mut self) -> io::Result<u64> {
        self.clock_reads += 1;
        self.monotonic_ns
            .pop_front()
            .ok_or_else(|| io::Error::other("clock script exhausted"))
    }

    fn epoll_create1(&mut self, flags: i32) -> io::Result<i64> {
        self.calls.push(FakeEpollCall::Create { flags });
        match self.create_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(self.create_fd),
        }
    }

    fn epoll_ctl(
        &mut self,
        epoll_fd: i32,
        op: i32,
        fd: i32,
        event: Option<LinuxEpollEvent>,
    ) -> io::Result<()> {
        self.calls.push(FakeEpollCall::Ctl {
            epoll_fd,
            op,
            fd,
            event,
        });
        match self.ctl_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(()),
        }
    }

    fn epoll_wait(
        &mut self,
        epoll_fd: i32,
        max_events: usize,
        timeout_ms: i32,
    ) -> io::Result<Vec<LinuxEpollEvent>> {
        self.calls.push(FakeEpollCall::Wait {
            epoll_fd,
            max_events,
            timeout_ms,
        });
        match self.wait_errors.pop_front() {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(self.wait_events.clone()),
        }
    }
}
