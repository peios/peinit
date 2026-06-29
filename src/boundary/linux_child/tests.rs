use std::collections::VecDeque;
use std::io;

use super::{
    ChildExitStatus, ChildReap, LinuxChildReapError, LinuxChildReapSyscallApi, LinuxWaitPid,
    WAIT_ANY_CHILD, WAIT_NONBLOCK_FLAGS, drain_linux_child_reaps, normalize_linux_wait_status,
};

#[test]
fn wait_status_normalizes_exit_and_signal() {
    assert_eq!(
        normalize_linux_wait_status(123, exit_status(7)).expect("exit"),
        ChildExitStatus::Exited { code: 7 },
    );
    assert_eq!(
        normalize_linux_wait_status(123, signal_status(libc::SIGTERM, false)).expect("signal"),
        ChildExitStatus::Signaled {
            signal: libc::SIGTERM,
            core_dumped: false,
        },
    );
    assert_eq!(
        normalize_linux_wait_status(123, signal_status(libc::SIGSEGV, true)).expect("core"),
        ChildExitStatus::Signaled {
            signal: libc::SIGSEGV,
            core_dumped: true,
        },
    );
}

#[test]
fn wait_status_rejects_stopped_and_continued_statuses() {
    assert!(matches!(
        normalize_linux_wait_status(123, stopped_status(libc::SIGSTOP)).expect_err("stopped"),
        LinuxChildReapError::UnsupportedStatus { pid: 123, .. },
    ));
    assert!(matches!(
        normalize_linux_wait_status(123, continued_status()).expect_err("continued"),
        LinuxChildReapError::UnsupportedStatus { pid: 123, .. },
    ));
}

#[test]
fn drain_reaps_children_until_no_status() {
    let mut syscalls = FakeChildReapSyscalls::new([
        Ok(LinuxWaitPid::Reaped {
            pid: 101,
            status: exit_status(0),
        }),
        Ok(LinuxWaitPid::Reaped {
            pid: 102,
            status: signal_status(libc::SIGKILL, false),
        }),
        Ok(LinuxWaitPid::NoStatus),
    ]);

    let reaped = drain_linux_child_reaps(&mut syscalls).expect("reap");

    assert_eq!(
        reaped,
        vec![
            ChildReap {
                pid: 101,
                status: ChildExitStatus::Exited { code: 0 },
            },
            ChildReap {
                pid: 102,
                status: ChildExitStatus::Signaled {
                    signal: libc::SIGKILL,
                    core_dumped: false,
                },
            },
        ],
    );
    assert_eq!(
        syscalls.calls,
        vec![
            FakeWaitpidCall {
                pid: WAIT_ANY_CHILD,
                options: WAIT_NONBLOCK_FLAGS,
            },
            FakeWaitpidCall {
                pid: WAIT_ANY_CHILD,
                options: WAIT_NONBLOCK_FLAGS,
            },
            FakeWaitpidCall {
                pid: WAIT_ANY_CHILD,
                options: WAIT_NONBLOCK_FLAGS,
            },
        ],
    );
}

#[test]
fn drain_treats_echild_as_empty() {
    let mut syscalls = FakeChildReapSyscalls::new([Ok(LinuxWaitPid::NoChildren)]);

    assert_eq!(
        drain_linux_child_reaps(&mut syscalls).expect("no children"),
        Vec::new(),
    );
}

#[test]
fn drain_reports_wait_error_and_pid_overflow() {
    let mut wait_error =
        FakeChildReapSyscalls::new([Err(io::Error::from(io::ErrorKind::Interrupted))]);
    assert!(matches!(
        drain_linux_child_reaps(&mut wait_error).expect_err("wait error"),
        LinuxChildReapError::Wait { .. },
    ));

    let mut pid_overflow = FakeChildReapSyscalls::new([Ok(LinuxWaitPid::Reaped {
        pid: i64::from(u32::MAX) + 1,
        status: exit_status(0),
    })]);
    assert!(matches!(
        drain_linux_child_reaps(&mut pid_overflow).expect_err("pid overflow"),
        LinuxChildReapError::PidOutOfRange { .. },
    ));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FakeWaitpidCall {
    pid: i32,
    options: i32,
}

#[derive(Debug)]
struct FakeChildReapSyscalls {
    results: VecDeque<io::Result<LinuxWaitPid>>,
    calls: Vec<FakeWaitpidCall>,
}

impl FakeChildReapSyscalls {
    fn new(results: impl IntoIterator<Item = io::Result<LinuxWaitPid>>) -> Self {
        Self {
            results: results.into_iter().collect(),
            calls: Vec::new(),
        }
    }
}

impl LinuxChildReapSyscallApi for FakeChildReapSyscalls {
    fn waitpid(&mut self, pid: i32, options: i32) -> io::Result<LinuxWaitPid> {
        self.calls.push(FakeWaitpidCall { pid, options });
        self.results.pop_front().expect("scripted waitpid result")
    }
}

fn exit_status(code: i32) -> i32 {
    (code & 0xff) << 8
}

fn signal_status(signal: i32, core_dumped: bool) -> i32 {
    signal | if core_dumped { 0x80 } else { 0 }
}

fn stopped_status(signal: i32) -> i32 {
    (signal << 8) | 0x7f
}

fn continued_status() -> i32 {
    0xffff
}
