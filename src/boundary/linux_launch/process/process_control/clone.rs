use std::io;
use std::os::fd::{FromRawFd, OwnedFd};

use crate::boundary::BoundaryError;

const CLONE_INTO_CGROUP: u64 = 0x200000000;

#[repr(C)]
#[derive(Debug, Default)]
struct CloneArgs {
    flags: u64,
    pidfd: u64,
    child_tid: u64,
    parent_tid: u64,
    exit_signal: u64,
    stack: u64,
    stack_size: u64,
    tls: u64,
    set_tid: u64,
    set_tid_size: u64,
    cgroup: u64,
}

pub(in crate::boundary::linux_launch::process) enum CloneProcessResult {
    Parent(ClonedProcess),
    Child,
}

pub(in crate::boundary::linux_launch::process) struct ClonedProcess {
    pub pid: libc::pid_t,
    pub pidfd: OwnedFd,
}

pub(in crate::boundary::linux_launch::process) fn clone_process_into_cgroup(
    cgroup_fd: i32,
) -> Result<CloneProcessResult, BoundaryError> {
    if cgroup_fd < 0 {
        return Err(BoundaryError::Process(format!(
            "invalid cgroup fd {cgroup_fd}",
        )));
    }
    let mut pidfd = -1i32;
    let mut args = CloneArgs {
        flags: libc::CLONE_PIDFD as u64 | CLONE_INTO_CGROUP,
        pidfd: (&mut pidfd as *mut i32) as u64,
        exit_signal: libc::SIGCHLD as u64,
        cgroup: cgroup_fd as u64,
        ..CloneArgs::default()
    };
    let pid = unsafe {
        libc::syscall(
            libc::SYS_clone3,
            &mut args as *mut CloneArgs,
            std::mem::size_of::<CloneArgs>(),
        )
    };
    if pid < 0 {
        return Err(BoundaryError::Process(format!(
            "clone3(CLONE_PIDFD|CLONE_INTO_CGROUP) failed: {}",
            io::Error::last_os_error(),
        )));
    }
    if pid == 0 {
        Ok(CloneProcessResult::Child)
    } else if pidfd < 0 {
        Err(BoundaryError::Process(format!(
            "clone3 returned pid {pid} without pidfd",
        )))
    } else {
        set_fd_cloexec(pidfd).map_err(|error| {
            BoundaryError::Process(format!(
                "set clone3 pidfd close-on-exec for pid {pid} failed: {error}",
            ))
        })?;
        Ok(CloneProcessResult::Parent(ClonedProcess {
            pid: pid as libc::pid_t,
            pidfd: unsafe { OwnedFd::from_raw_fd(pidfd) },
        }))
    }
}

fn set_fd_cloexec(fd: i32) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
