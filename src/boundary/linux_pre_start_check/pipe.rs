use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use crate::boundary::BoundaryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReadOutcome {
    Bytes(usize),
    WouldBlock,
}

pub(super) fn create_result_pipe() -> Result<(OwnedFd, OwnedFd), BoundaryError> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(BoundaryError::Process(format!(
            "pipe2(filesystem-check-result) failed: {}",
            io::Error::last_os_error(),
        )));
    }
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    set_nonblocking(read.as_raw_fd()).map_err(|error| {
        BoundaryError::Process(format!(
            "set filesystem check result pipe nonblocking failed: {error}",
        ))
    })?;
    Ok((read, write))
}

pub(super) fn read_fd(fd: i32, bytes: &mut [u8]) -> Result<ReadOutcome, BoundaryError> {
    let read = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
    if read >= 0 {
        return Ok(ReadOutcome::Bytes(read as usize));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Ok(ReadOutcome::WouldBlock)
    } else {
        Err(BoundaryError::Process(format!(
            "read filesystem check helper fd {fd} failed: {error}",
        )))
    }
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
