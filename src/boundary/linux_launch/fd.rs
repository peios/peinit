use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use crate::boundary::BoundaryError;

pub(super) fn create_pipe(label: &str) -> Result<(OwnedFd, OwnedFd), BoundaryError> {
    let mut fds = [0; 2];
    let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if result != 0 {
        return Err(BoundaryError::Process(format!(
            "pipe2({label}) failed: {}",
            io::Error::last_os_error(),
        )));
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

pub(super) fn create_read_nonblocking_pipe(
    label: &str,
) -> Result<(OwnedFd, OwnedFd), BoundaryError> {
    let (read, write) = create_pipe(label)?;
    set_nonblocking(read.as_raw_fd()).map_err(|error| {
        BoundaryError::Process(format!("set {label} read pipe nonblocking failed: {error}"))
    })?;
    Ok((read, write))
}

pub(super) fn create_output_pipe(
    label: &str,
    capacity_bytes: usize,
) -> Result<(OwnedFd, OwnedFd), BoundaryError> {
    let (read, write) = create_read_nonblocking_pipe(label)?;
    set_pipe_capacity(read.as_raw_fd(), capacity_bytes).map_err(|error| {
        BoundaryError::Process(format!("set {label} pipe capacity failed: {error}"))
    })?;
    Ok((read, write))
}

fn set_nonblocking(fd: i32) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_pipe_capacity(fd: i32, capacity_bytes: usize) -> io::Result<()> {
    if capacity_bytes == 0 || capacity_bytes > i32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid pipe capacity {capacity_bytes}"),
        ));
    }
    let result = unsafe { libc::fcntl(fd, libc::F_SETPIPE_SZ, capacity_bytes as i32) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(super) fn close_fd(fd: i32) {
    unsafe {
        libc::close(fd);
    }
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;

    use super::{create_output_pipe, create_pipe, create_read_nonblocking_pipe};

    #[test]
    fn output_pipe_has_nonblocking_parent_read_and_blocking_child_write() {
        let (read, write) = create_output_pipe(
            "test-output",
            crate::logging::DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
        )
        .expect("pipe");

        assert!(fd_flags(read.as_raw_fd()) & libc::O_NONBLOCK != 0);
        assert_eq!(fd_flags(write.as_raw_fd()) & libc::O_NONBLOCK, 0);
    }

    #[test]
    fn output_pipe_uses_requested_capacity_for_backpressure() {
        let requested = 4096;
        let (read, _write) = create_output_pipe("test-output", requested).expect("pipe");

        assert!(pipe_capacity(read.as_raw_fd()) >= requested);
    }

    #[test]
    fn read_nonblocking_pipe_keeps_close_on_exec_on_both_ends() {
        let (read, write) = create_read_nonblocking_pipe("test-status").expect("pipe");

        assert!(fd_descriptor_flags(read.as_raw_fd()) & libc::FD_CLOEXEC != 0);
        assert!(fd_descriptor_flags(write.as_raw_fd()) & libc::FD_CLOEXEC != 0);
    }

    #[test]
    fn plain_pipe_keeps_close_on_exec_on_both_ends() {
        let (read, write) = create_pipe("test-plain").expect("pipe");

        assert!(fd_descriptor_flags(read.as_raw_fd()) & libc::FD_CLOEXEC != 0);
        assert!(fd_descriptor_flags(write.as_raw_fd()) & libc::FD_CLOEXEC != 0);
    }

    fn fd_flags(fd: i32) -> i32 {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert!(flags >= 0);
        flags
    }

    fn fd_descriptor_flags(fd: i32) -> i32 {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0);
        flags
    }

    fn pipe_capacity(fd: i32) -> usize {
        let capacity = unsafe { libc::fcntl(fd, libc::F_GETPIPE_SZ) };
        assert!(capacity > 0);
        capacity as usize
    }
}
