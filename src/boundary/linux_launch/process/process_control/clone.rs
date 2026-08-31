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
        return Ok(CloneProcessResult::Child);
    }
    // From here the child exists and is already running in the service's
    // cgroup with the service's token. Every failure below is a failure to
    // take a handle on it, so each one kills the cgroup before returning:
    // returning an error and walking away leaves a process peinit cannot
    // supervise, cannot stop, and has no record of.
    //
    // The zero-delay ServiceTree cleanup that follows a ParentSetupFailure
    // makes that worse rather than better. It runs immediately, before the
    // child could plausibly have exec'd or exited, so it is near-certain to
    // find the cgroup populated and take the *leak* branch — recording the
    // tree as unreclaimable and bumping the generation, so the next start
    // builds a fresh tree beside the orphan and leaves it running until
    // reboot (PEI-354).
    if pidfd < 0 {
        kill_cloned_cgroup(cgroup_fd);
        return Err(BoundaryError::Process(format!(
            "clone3 returned pid {pid} without pidfd",
        )));
    }
    if let Err(error) = set_fd_cloexec(pidfd) {
        kill_cloned_cgroup(cgroup_fd);
        // The descriptor was never wrapped in an OwnedFd, so nothing else will
        // close it.
        unsafe { libc::close(pidfd) };
        return Err(BoundaryError::Process(format!(
            "set clone3 pidfd close-on-exec for pid {pid} failed: {error}",
        )));
    }
    Ok(CloneProcessResult::Parent(ClonedProcess {
        pid: pid as libc::pid_t,
        pidfd: unsafe { OwnedFd::from_raw_fd(pidfd) },
    }))
}

/// Kill everything in the cgroup the child was cloned into.
///
/// Best-effort by construction. This runs on a path that is already returning
/// an error, and a failure to kill is less useful to the caller than the
/// failure that brought us here — but leaving the process alive is not an
/// option, so it is attempted unconditionally.
///
/// `openat` relative to the cgroup directory fd rather than by path: the fd is
/// the one the clone itself used, so there is no window in which the tree
/// could have been replaced underneath us, and no path to reconstruct.
pub(super) fn kill_cloned_cgroup(cgroup_fd: i32) {
    const CGROUP_KILL: &[u8] = b"cgroup.kill\0";
    let file = unsafe {
        libc::openat(
            cgroup_fd,
            CGROUP_KILL.as_ptr().cast(),
            libc::O_WRONLY | libc::O_CLOEXEC,
        )
    };
    if file < 0 {
        return;
    }
    unsafe {
        libc::write(file, b"1".as_ptr().cast(), 1);
        libc::close(file);
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
