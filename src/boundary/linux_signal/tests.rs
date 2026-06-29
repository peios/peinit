use std::collections::VecDeque;
use std::io;

use crate::boundary::linux_signal::{
    LinuxSignalFdRead, LinuxSignalMask, PID1_SIGNALFD_CREATE_FD, PID1_SIGNALFD_FLAGS,
    Pid1SignalFdSetupError, Pid1SignalFdSyscalls, setup_pid1_signalfd,
};
use crate::shutdown::ShutdownSignal;

use super::read_pid1_signalfd;

#[test]
fn pid1_signal_mask_contains_all_blockable_linux_signals() {
    let mask = LinuxSignalMask::all_blockable();

    assert!(mask.contains(libc::SIGINT));
    assert!(mask.contains(libc::SIGTERM));
    assert!(mask.contains(libc::SIGCHLD));
    assert!(!mask.contains(libc::SIGKILL));
    assert!(!mask.contains(libc::SIGSTOP));
    assert!(!mask.contains(0));
    assert!(!mask.contains(65));
}

#[test]
fn setup_pid1_signalfd_blocks_same_mask_used_for_signalfd_creation() {
    let mut syscalls = FakeSignalSyscalls::default();

    let setup = setup_pid1_signalfd(&mut syscalls).expect("setup signalfd");

    assert_eq!(setup.fd, 17);
    assert_eq!(setup.flags, PID1_SIGNALFD_FLAGS);
    assert_eq!(
        syscalls.calls,
        vec![
            FakeSignalCall::RtSigprocmask {
                how: libc::SIG_BLOCK,
                mask: LinuxSignalMask::all_blockable(),
            },
            FakeSignalCall::Signalfd4 {
                fd: PID1_SIGNALFD_CREATE_FD,
                mask: LinuxSignalMask::all_blockable(),
                flags: PID1_SIGNALFD_FLAGS,
            },
        ],
    );
}

#[test]
fn setup_pid1_signalfd_fails_closed_when_signal_blocking_fails() {
    let mut syscalls = FakeSignalSyscalls {
        rt_sigprocmask_error: Some(io::ErrorKind::PermissionDenied),
        ..FakeSignalSyscalls::default()
    };

    let err = setup_pid1_signalfd(&mut syscalls).expect_err("sigprocmask failure");

    assert!(matches!(
        err,
        Pid1SignalFdSetupError::Sigprocmask {
            how: libc::SIG_BLOCK,
            ..
        },
    ));
    assert_eq!(syscalls.calls.len(), 1);
}

#[test]
fn setup_pid1_signalfd_reports_signalfd_creation_failure() {
    let mut syscalls = FakeSignalSyscalls {
        signalfd4_error: Some(io::ErrorKind::Other),
        ..FakeSignalSyscalls::default()
    };

    let err = setup_pid1_signalfd(&mut syscalls).expect_err("signalfd failure");

    assert!(matches!(
        err,
        Pid1SignalFdSetupError::Signalfd {
            fd: PID1_SIGNALFD_CREATE_FD,
            flags: PID1_SIGNALFD_FLAGS,
            ..
        },
    ));
}

#[test]
fn setup_pid1_signalfd_rejects_fd_values_outside_i32_range() {
    let mut syscalls = FakeSignalSyscalls {
        signalfd4_fd: i64::from(i32::MAX) + 1,
        ..FakeSignalSyscalls::default()
    };

    let err = setup_pid1_signalfd(&mut syscalls).expect_err("out of range fd");

    assert!(matches!(
        err,
        Pid1SignalFdSetupError::FdOutOfRange {
            returned_fd,
        } if returned_fd == i64::from(i32::MAX) + 1
    ));
}

#[test]
fn read_pid1_signalfd_maps_shutdown_signals() {
    let mut syscalls = FakeSignalSyscalls {
        reads: VecDeque::from([
            Ok(Some(signalfd_info(libc::SIGINT))),
            Ok(Some(signalfd_info(libc::SIGTERM))),
        ]),
        ..FakeSignalSyscalls::default()
    };

    assert_eq!(
        read_pid1_signalfd(&mut syscalls, 17).expect("sigint"),
        LinuxSignalFdRead::Shutdown(ShutdownSignal::Sigint),
    );
    assert_eq!(
        read_pid1_signalfd(&mut syscalls, 17).expect("sigterm"),
        LinuxSignalFdRead::Shutdown(ShutdownSignal::Sigterm),
    );
}

#[test]
fn read_pid1_signalfd_reports_other_signals_and_would_block() {
    let mut syscalls = FakeSignalSyscalls {
        reads: VecDeque::from([Ok(Some(signalfd_info(libc::SIGCHLD))), Ok(None)]),
        ..FakeSignalSyscalls::default()
    };

    assert_eq!(
        read_pid1_signalfd(&mut syscalls, 17).expect("sigchld"),
        LinuxSignalFdRead::Other {
            signal: libc::SIGCHLD,
        },
    );
    assert_eq!(
        read_pid1_signalfd(&mut syscalls, 17).expect("would block"),
        LinuxSignalFdRead::WouldBlock,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeSignalCall {
    RtSigprocmask {
        how: i32,
        mask: LinuxSignalMask,
    },
    Signalfd4 {
        fd: i32,
        mask: LinuxSignalMask,
        flags: i32,
    },
    Read {
        fd: i32,
    },
}

#[derive(Debug)]
struct FakeSignalSyscalls {
    rt_sigprocmask_error: Option<io::ErrorKind>,
    signalfd4_error: Option<io::ErrorKind>,
    signalfd4_fd: i64,
    reads: VecDeque<io::Result<Option<libc::signalfd_siginfo>>>,
    calls: Vec<FakeSignalCall>,
}

impl Default for FakeSignalSyscalls {
    fn default() -> Self {
        Self {
            rt_sigprocmask_error: None,
            signalfd4_error: None,
            signalfd4_fd: 17,
            reads: VecDeque::new(),
            calls: Vec::new(),
        }
    }
}

impl Pid1SignalFdSyscalls for FakeSignalSyscalls {
    fn rt_sigprocmask(&mut self, how: i32, mask: &LinuxSignalMask) -> io::Result<()> {
        self.calls
            .push(FakeSignalCall::RtSigprocmask { how, mask: *mask });
        match self.rt_sigprocmask_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(()),
        }
    }

    fn signalfd4(&mut self, fd: i32, mask: &LinuxSignalMask, flags: i32) -> io::Result<i64> {
        self.calls.push(FakeSignalCall::Signalfd4 {
            fd,
            mask: *mask,
            flags,
        });
        match self.signalfd4_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(self.signalfd4_fd),
        }
    }

    fn read_signalfd(&mut self, fd: i32) -> io::Result<Option<libc::signalfd_siginfo>> {
        self.calls.push(FakeSignalCall::Read { fd });
        self.reads.pop_front().unwrap_or(Ok(None))
    }

    fn close_fd(&mut self, _fd: i32) -> io::Result<()> {
        Ok(())
    }
}

fn signalfd_info(signal: i32) -> libc::signalfd_siginfo {
    let mut info = unsafe { std::mem::zeroed::<libc::signalfd_siginfo>() };
    info.ssi_signo = signal as u32;
    info
}
