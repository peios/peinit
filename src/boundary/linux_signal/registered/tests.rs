use std::io;

use crate::boundary::linux_epoll::{LinuxEpollEvent, LinuxEpollSyscallApi};
use crate::boundary::linux_signal::{
    LinuxSignalMask, PID1_SIGNALFD_CREATE_FD, PID1_SIGNALFD_FLAGS,
    Pid1SignalFdRegisteredSetupError, Pid1SignalFdSyscalls, setup_pid1_signalfd_registered,
};

#[test]
fn registered_pid1_signalfd_blocks_signals_creates_fd_and_registers_read_interest() {
    let mut syscalls = FakeRegisteredSignalSyscalls::default();

    let setup = setup_pid1_signalfd_registered(&mut syscalls, 30, 700).expect("registered setup");

    assert_eq!(setup.signal.fd, 17);
    assert_eq!(setup.epoll_fd, 30);
    assert_eq!(setup.token, 700);
    assert_eq!(
        syscalls.calls,
        vec![
            FakeRegisteredCall::RtSigprocmask {
                how: libc::SIG_BLOCK,
                mask: LinuxSignalMask::all_blockable(),
            },
            FakeRegisteredCall::Signalfd4 {
                fd: PID1_SIGNALFD_CREATE_FD,
                mask: LinuxSignalMask::all_blockable(),
                flags: PID1_SIGNALFD_FLAGS,
            },
            FakeRegisteredCall::EpollCtl {
                epoll_fd: 30,
                op: libc::EPOLL_CTL_ADD,
                fd: 17,
                event: Some(LinuxEpollEvent::read(700)),
            },
        ],
    );
}

#[test]
fn registered_pid1_signalfd_fails_before_registration_when_signalfd_setup_fails() {
    let mut syscalls = FakeRegisteredSignalSyscalls {
        signalfd4_error: Some(io::ErrorKind::Other),
        ..FakeRegisteredSignalSyscalls::default()
    };

    let err =
        setup_pid1_signalfd_registered(&mut syscalls, 30, 700).expect_err("signalfd setup failure");

    assert!(matches!(err, Pid1SignalFdRegisteredSetupError::Signal(_)));
    assert!(
        !syscalls
            .calls
            .iter()
            .any(|call| matches!(call, FakeRegisteredCall::EpollCtl { .. }))
    );
}

#[test]
fn registered_pid1_signalfd_closes_signal_fd_when_epoll_registration_fails() {
    let mut syscalls = FakeRegisteredSignalSyscalls {
        epoll_ctl_error: Some(io::ErrorKind::InvalidInput),
        ..FakeRegisteredSignalSyscalls::default()
    };

    let err =
        setup_pid1_signalfd_registered(&mut syscalls, 30, 700).expect_err("registration failure");

    assert!(matches!(
        err,
        Pid1SignalFdRegisteredSetupError::Register {
            close_error: None,
            ..
        },
    ));
    assert!(matches!(
        syscalls.calls.last(),
        Some(FakeRegisteredCall::Close { fd: 17 }),
    ));
}

#[test]
fn registered_pid1_signalfd_reports_cleanup_failure_after_registration_failure() {
    let mut syscalls = FakeRegisteredSignalSyscalls {
        epoll_ctl_error: Some(io::ErrorKind::InvalidInput),
        close_error: Some(io::ErrorKind::PermissionDenied),
        ..FakeRegisteredSignalSyscalls::default()
    };

    let err =
        setup_pid1_signalfd_registered(&mut syscalls, 30, 700).expect_err("registration failure");

    assert!(matches!(
        err,
        Pid1SignalFdRegisteredSetupError::Register {
            close_error: Some(_),
            ..
        },
    ));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeRegisteredCall {
    RtSigprocmask {
        how: i32,
        mask: LinuxSignalMask,
    },
    Signalfd4 {
        fd: i32,
        mask: LinuxSignalMask,
        flags: i32,
    },
    EpollCtl {
        epoll_fd: i32,
        op: i32,
        fd: i32,
        event: Option<LinuxEpollEvent>,
    },
    Close {
        fd: i32,
    },
}

#[derive(Debug)]
struct FakeRegisteredSignalSyscalls {
    signalfd4_fd: i64,
    signalfd4_error: Option<io::ErrorKind>,
    epoll_ctl_error: Option<io::ErrorKind>,
    close_error: Option<io::ErrorKind>,
    calls: Vec<FakeRegisteredCall>,
}

impl Default for FakeRegisteredSignalSyscalls {
    fn default() -> Self {
        Self {
            signalfd4_fd: 17,
            signalfd4_error: None,
            epoll_ctl_error: None,
            close_error: None,
            calls: Vec::new(),
        }
    }
}

impl Pid1SignalFdSyscalls for FakeRegisteredSignalSyscalls {
    fn rt_sigprocmask(&mut self, how: i32, mask: &LinuxSignalMask) -> io::Result<()> {
        self.calls
            .push(FakeRegisteredCall::RtSigprocmask { how, mask: *mask });
        Ok(())
    }

    fn signalfd4(&mut self, fd: i32, mask: &LinuxSignalMask, flags: i32) -> io::Result<i64> {
        self.calls.push(FakeRegisteredCall::Signalfd4 {
            fd,
            mask: *mask,
            flags,
        });
        match self.signalfd4_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(self.signalfd4_fd),
        }
    }

    fn read_signalfd(&mut self, _fd: i32) -> io::Result<Option<libc::signalfd_siginfo>> {
        Ok(None)
    }

    fn close_fd(&mut self, fd: i32) -> io::Result<()> {
        self.calls.push(FakeRegisteredCall::Close { fd });
        match self.close_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(()),
        }
    }
}

impl LinuxEpollSyscallApi for FakeRegisteredSignalSyscalls {
    fn epoll_create1(&mut self, _flags: i32) -> io::Result<i64> {
        unreachable!("registered signalfd setup receives an existing epoll fd")
    }

    fn epoll_ctl(
        &mut self,
        epoll_fd: i32,
        op: i32,
        fd: i32,
        event: Option<LinuxEpollEvent>,
    ) -> io::Result<()> {
        self.calls.push(FakeRegisteredCall::EpollCtl {
            epoll_fd,
            op,
            fd,
            event,
        });
        match self.epoll_ctl_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(()),
        }
    }

    fn epoll_wait(
        &mut self,
        _epoll_fd: i32,
        _max_events: usize,
        _timeout_ms: i32,
    ) -> io::Result<Vec<LinuxEpollEvent>> {
        Ok(Vec::new())
    }
}
