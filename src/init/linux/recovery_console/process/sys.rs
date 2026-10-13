use std::io;

pub(super) fn pipe_cloexec() -> io::Result<(i32, i32)> {
    let mut fds = [-1_i32; 2];
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok((fds[0], fds[1]))
    }
}

pub(super) fn close_raw_fd(fd: i32) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}

/// Make `to` an inheritable copy of `from`, returning `false` on failure.
/// `dup2(fd, fd)` leaves close-on-exec alone, so a close-on-exec console
/// already sitting on a standard slot has the flag cleared explicitly.
pub(super) fn dup_inheritable(from: i32, to: i32) -> bool {
    if from != to {
        return unsafe { libc::dup2(from, to) } >= 0;
    }
    let flags = unsafe { libc::fcntl(to, libc::F_GETFD) };
    flags >= 0 && unsafe { libc::fcntl(to, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } == 0
}

pub(super) fn last_errno() -> i32 {
    io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

#[cfg(test)]
mod tests {
    use super::{close_raw_fd, dup_inheritable, pipe_cloexec};

    fn cloexec(fd: i32) -> bool {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0);
        flags & libc::FD_CLOEXEC != 0
    }

    /// PEI-331: the console handle is close-on-exec. Duplicated onto
    /// another slot or already on its own, the target must be inheritable.
    #[test]
    fn the_console_target_is_inheritable_even_when_it_is_the_source() {
        let (read, write) = pipe_cloexec().expect("pipe");
        assert!(cloexec(read));
        assert!(dup_inheritable(read, read));
        assert!(!cloexec(read));

        let target = unsafe { libc::fcntl(write, libc::F_DUPFD_CLOEXEC, 300) };
        assert!(target >= 0);
        assert!(dup_inheritable(write, target));
        assert!(!cloexec(target));
        assert!(cloexec(write), "the source keeps its own flag");

        assert!(!dup_inheritable(-1, -1));
        for fd in [read, write, target] {
            close_raw_fd(fd);
        }
    }
}
