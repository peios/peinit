use std::io;

use crate::boundary::linux_timer::{
    LinuxTimerFdArmError, LinuxTimerFdCreateError, LinuxTimerFdRead, LinuxTimerFdSyscallApi,
    LinuxTimerSpec, TIMERFD_ABSOLUTE_FLAGS, TIMERFD_CREATE_CLOCK, TIMERFD_CREATE_FLAGS,
    TIMERFD_REALTIME_ABSOLUTE_FLAGS, TIMERFD_REALTIME_CREATE_CLOCK, create_linux_monotonic_timerfd,
    create_linux_realtime_timerfd, linux_timer_spec_from_deadline_ns, read_linux_timerfd,
    set_linux_timerfd_absolute, set_linux_timerfd_realtime_absolute,
};

#[test]
fn timer_spec_converts_monotonic_deadline_ns_to_absolute_oneshot_spec() {
    assert_eq!(
        linux_timer_spec_from_deadline_ns(12_345_678_901),
        LinuxTimerSpec::absolute_oneshot(12, 345_678_901),
    );
}

#[test]
fn create_linux_monotonic_timerfd_uses_nonblocking_close_on_exec_monotonic_timer() {
    let mut syscalls = FakeTimerFdSyscalls::default();

    let fd = create_linux_monotonic_timerfd(&mut syscalls).expect("create timerfd");

    assert_eq!(fd, 33);
    assert_eq!(
        syscalls.calls,
        vec![FakeTimerCall::Create {
            clock_id: TIMERFD_CREATE_CLOCK,
            flags: TIMERFD_CREATE_FLAGS,
        }],
    );
}

#[test]
fn create_linux_realtime_timerfd_uses_nonblocking_close_on_exec_realtime_timer() {
    let mut syscalls = FakeTimerFdSyscalls::default();

    let fd = create_linux_realtime_timerfd(&mut syscalls).expect("create timerfd");

    assert_eq!(fd, 33);
    assert_eq!(
        syscalls.calls,
        vec![FakeTimerCall::Create {
            clock_id: TIMERFD_REALTIME_CREATE_CLOCK,
            flags: TIMERFD_CREATE_FLAGS,
        }],
    );
}

#[test]
fn create_linux_monotonic_timerfd_reports_create_failure_and_fd_overflow() {
    let mut syscalls = FakeTimerFdSyscalls {
        create_error: Some(io::ErrorKind::PermissionDenied),
        ..FakeTimerFdSyscalls::default()
    };
    assert!(matches!(
        create_linux_monotonic_timerfd(&mut syscalls).expect_err("create failure"),
        LinuxTimerFdCreateError::Create {
            clock_id: TIMERFD_CREATE_CLOCK,
            flags: TIMERFD_CREATE_FLAGS,
            ..
        },
    ));

    let mut syscalls = FakeTimerFdSyscalls {
        create_fd: i64::from(i32::MAX) + 1,
        ..FakeTimerFdSyscalls::default()
    };
    assert!(matches!(
        create_linux_monotonic_timerfd(&mut syscalls).expect_err("fd overflow"),
        LinuxTimerFdCreateError::FdOutOfRange { returned_fd }
            if returned_fd == i64::from(i32::MAX) + 1
    ));
}

#[test]
fn set_linux_timerfd_absolute_arms_oneshot_absolute_timer() {
    let mut syscalls = FakeTimerFdSyscalls::default();

    set_linux_timerfd_absolute(&mut syscalls, 33, 98_765_432_100).expect("arm timer");

    assert_eq!(
        syscalls.calls,
        vec![FakeTimerCall::SetTime {
            fd: 33,
            flags: TIMERFD_ABSOLUTE_FLAGS,
            spec: LinuxTimerSpec::absolute_oneshot(98, 765_432_100),
        }],
    );
}

#[test]
fn set_linux_timerfd_realtime_absolute_arms_cancel_on_set_absolute_timer() {
    let mut syscalls = FakeTimerFdSyscalls::default();

    set_linux_timerfd_realtime_absolute(&mut syscalls, 33, 98_765_432_100).expect("arm timer");

    assert_eq!(
        syscalls.calls,
        vec![FakeTimerCall::SetTime {
            fd: 33,
            flags: TIMERFD_REALTIME_ABSOLUTE_FLAGS,
            spec: LinuxTimerSpec::absolute_oneshot(98, 765_432_100),
        }],
    );
}

#[test]
fn set_linux_timerfd_absolute_reports_settime_failures() {
    let mut syscalls = FakeTimerFdSyscalls {
        settime_error: Some(io::ErrorKind::InvalidInput),
        ..FakeTimerFdSyscalls::default()
    };
    assert!(matches!(
        set_linux_timerfd_absolute(&mut syscalls, 33, 1_000_000_000).expect_err("settime failure"),
        LinuxTimerFdArmError::SetTime {
            fd: 33,
            flags: TIMERFD_ABSOLUTE_FLAGS,
            spec: LinuxTimerSpec {
                initial_sec: 1,
                initial_nsec: 0,
                ..
            },
            ..
        },
    ));
}

#[test]
fn read_linux_timerfd_reports_expirations_and_would_block() {
    let mut syscalls = FakeTimerFdSyscalls {
        read_result: Ok(Some(3)),
        ..FakeTimerFdSyscalls::default()
    };
    assert_eq!(
        read_linux_timerfd(&mut syscalls, 33).expect("read"),
        LinuxTimerFdRead::Expired { expirations: 3 },
    );

    let mut syscalls = FakeTimerFdSyscalls {
        read_result: Ok(None),
        ..FakeTimerFdSyscalls::default()
    };
    assert_eq!(
        read_linux_timerfd(&mut syscalls, 33).expect("would block"),
        LinuxTimerFdRead::WouldBlock,
    );
}

#[test]
fn read_linux_timerfd_reports_realtime_clock_cancellation() {
    let mut syscalls = FakeTimerFdSyscalls {
        read_result: Err(io::Error::from_raw_os_error(libc::ECANCELED)),
        ..FakeTimerFdSyscalls::default()
    };

    assert_eq!(
        read_linux_timerfd(&mut syscalls, 33).expect("read"),
        LinuxTimerFdRead::Canceled,
    );
}

#[test]
fn read_linux_timerfd_reports_read_failure() {
    let mut syscalls = FakeTimerFdSyscalls {
        read_result: Err(io::Error::from(io::ErrorKind::Interrupted)),
        ..FakeTimerFdSyscalls::default()
    };

    assert!(matches!(
        read_linux_timerfd(&mut syscalls, 33).expect_err("read failure"),
        crate::boundary::linux_timer::LinuxTimerFdReadError::Read(_),
    ));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeTimerCall {
    Create {
        clock_id: i32,
        flags: i32,
    },
    SetTime {
        fd: i32,
        flags: i32,
        spec: LinuxTimerSpec,
    },
    Read {
        fd: i32,
    },
}

#[derive(Debug)]
struct FakeTimerFdSyscalls {
    create_fd: i64,
    create_error: Option<io::ErrorKind>,
    settime_error: Option<io::ErrorKind>,
    read_result: io::Result<Option<u64>>,
    calls: Vec<FakeTimerCall>,
}

impl Default for FakeTimerFdSyscalls {
    fn default() -> Self {
        Self {
            create_fd: 33,
            create_error: None,
            settime_error: None,
            read_result: Ok(None),
            calls: Vec::new(),
        }
    }
}

impl LinuxTimerFdSyscallApi for FakeTimerFdSyscalls {
    fn timerfd_create(&mut self, clock_id: i32, flags: i32) -> io::Result<i64> {
        self.calls.push(FakeTimerCall::Create { clock_id, flags });
        match self.create_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(self.create_fd),
        }
    }

    fn timerfd_settime(&mut self, fd: i32, flags: i32, spec: LinuxTimerSpec) -> io::Result<()> {
        self.calls.push(FakeTimerCall::SetTime { fd, flags, spec });
        match self.settime_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(()),
        }
    }

    fn read_timerfd(&mut self, fd: i32) -> io::Result<Option<u64>> {
        self.calls.push(FakeTimerCall::Read { fd });
        match &self.read_result {
            Ok(result) => Ok(*result),
            Err(error) => match error.raw_os_error() {
                Some(errno) => Err(io::Error::from_raw_os_error(errno)),
                None => Err(io::Error::new(error.kind(), error.to_string())),
            },
        }
    }
}
