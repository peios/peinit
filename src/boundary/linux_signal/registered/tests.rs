use std::io;

use crate::boundary::linux_epoll::{LinuxEpollEvent, LinuxEpollSyscallApi};
use crate::boundary::linux_signal::{
    LinuxSignalMask, PID1_SIGNALFD_CREATE_FD, PID1_SIGNALFD_FLAGS,
    Pid1SignalFdRegisteredSetupError, Pid1SignalFdSetupError, Pid1SignalFdSyscalls,
    setup_pid1_signalfd_registered,
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

// Peinit TRM §12.3: if any part of the signalfd setup fails — blocking the
// signals, creating the descriptor, retaining it, registering it with the
// event loop — peinit fails closed, with no fallback to asynchronous
// handlers. Each failure point in turn: every one comes back as an error,
// nothing later in the sequence is attempted, and a descriptor that was
// created is closed rather than left behind. There is nothing to fall back
// *to*: the syscall surface this runs on has no way to install a handler, so
// an error is the only outcome the setup has other than a registered fd.
#[test]
fn every_signalfd_setup_failure_fails_closed() {
    let registered = |calls: &[FakeRegisteredCall]| {
        calls
            .iter()
            .any(|call| matches!(call, FakeRegisteredCall::EpollCtl { .. }))
    };

    // Blocking.
    let mut blocking = FakeRegisteredSignalSyscalls {
        rt_sigprocmask_error: Some(io::ErrorKind::PermissionDenied),
        ..FakeRegisteredSignalSyscalls::default()
    };
    let err = setup_pid1_signalfd_registered(&mut blocking, 30, 700).expect_err("blocking");
    assert!(matches!(
        err,
        Pid1SignalFdRegisteredSetupError::Signal(Pid1SignalFdSetupError::Sigprocmask { .. }),
    ));
    assert_eq!(blocking.calls.len(), 1, "nothing followed the failed mask");

    // Creating the descriptor.
    let mut creating = FakeRegisteredSignalSyscalls {
        signalfd4_error: Some(io::ErrorKind::Other),
        ..FakeRegisteredSignalSyscalls::default()
    };
    let err = setup_pid1_signalfd_registered(&mut creating, 30, 700).expect_err("creating");
    assert!(matches!(
        err,
        Pid1SignalFdRegisteredSetupError::Signal(Pid1SignalFdSetupError::Signalfd { .. }),
    ));
    assert!(!registered(&creating.calls), "nothing was registered");

    // Retaining it: a descriptor that does not fit the fd type is refused.
    let mut retaining = FakeRegisteredSignalSyscalls {
        signalfd4_fd: i64::from(i32::MAX) + 1,
        ..FakeRegisteredSignalSyscalls::default()
    };
    let err = setup_pid1_signalfd_registered(&mut retaining, 30, 700).expect_err("retaining");
    assert!(matches!(
        err,
        Pid1SignalFdRegisteredSetupError::Signal(Pid1SignalFdSetupError::FdOutOfRange { .. }),
    ));
    assert!(!registered(&retaining.calls), "nothing was registered");

    // Registering it with the event loop.
    let mut registering = FakeRegisteredSignalSyscalls {
        epoll_ctl_error: Some(io::ErrorKind::InvalidInput),
        ..FakeRegisteredSignalSyscalls::default()
    };
    let err =
        setup_pid1_signalfd_registered(&mut registering, 30, 700).expect_err("registering");
    assert!(matches!(
        err,
        Pid1SignalFdRegisteredSetupError::Register {
            close_error: None,
            ..
        },
    ));
    assert!(
        matches!(
            registering.calls.last(),
            Some(FakeRegisteredCall::Close { fd: 17 })
        ),
        "the descriptor that could not be registered was closed",
    );
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
    rt_sigprocmask_error: Option<io::ErrorKind>,
    signalfd4_fd: i64,
    signalfd4_error: Option<io::ErrorKind>,
    epoll_ctl_error: Option<io::ErrorKind>,
    close_error: Option<io::ErrorKind>,
    calls: Vec<FakeRegisteredCall>,
}

impl Default for FakeRegisteredSignalSyscalls {
    fn default() -> Self {
        Self {
            rt_sigprocmask_error: None,
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
        match self.rt_sigprocmask_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(()),
        }
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
