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
    assert!(matches!(
        wait_linux_epoll(&mut syscalls, 23, 0, 250).expect_err("empty buffer"),
        LinuxEpollWaitError::InvalidMaxEvents,
    ));
}

#[test]
fn wait_linux_epoll_reports_wait_failure() {
    let mut syscalls = FakeEpollSyscalls {
        wait_error: Some(io::ErrorKind::Interrupted),
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
    wait_error: Option<io::ErrorKind>,
    calls: Vec<FakeEpollCall>,
}

impl Default for FakeEpollSyscalls {
    fn default() -> Self {
        Self {
            create_fd: 23,
            create_error: None,
            ctl_error: None,
            wait_events: Vec::new(),
            wait_error: None,
            calls: Vec::new(),
        }
    }
}

impl LinuxEpollSyscallApi for FakeEpollSyscalls {
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
        match self.wait_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(self.wait_events.clone()),
        }
    }
}
