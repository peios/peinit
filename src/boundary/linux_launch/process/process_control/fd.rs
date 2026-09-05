use std::os::fd::{FromRawFd, OwnedFd};
use std::path::Path;

use peios::file::{FileAccess, OpenOptions};
use peios::token::Token;

use crate::boundary::{BoundaryError, TokenHandle};

pub(in crate::boundary::linux_launch::process) fn token_from_handle(
    handle: TokenHandle,
) -> Result<Token, BoundaryError> {
    if handle.fd < 0 {
        return Err(BoundaryError::Token(format!(
            "invalid token fd {} for {}",
            handle.fd, handle.identity,
        )));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(handle.fd) };
    Ok(Token::from(fd))
}

pub(in crate::boundary::linux_launch::process) fn open_dev_null() -> Result<OwnedFd, BoundaryError>
{
    let file = OpenOptions::new()
        .desired_access(FileAccess::READ_DATA | FileAccess::WRITE_DATA)
        .open(None, Path::new("/dev/null"))
        .map_err(|error| BoundaryError::Process(format!("open /dev/null failed: {error}")))?;
    Ok(file.into())
}

/// Open a service's terminal read/write, from its `TTYPath`. The child dups
/// this onto stdin/stdout/stderr in place of the daemon `/dev/null` + log-pipe
/// wiring and adopts it as its controlling terminal.
///
/// READ_ATTRIBUTES is on the mask because every process in the session
/// inherits this descriptor as its terminal, and a KACS handle answers
/// `fstat` only with that right. Without it `ttyname()` fails, `tty` says
/// "not a tty", and `login` cannot tell a serial line from a virtual console
/// when choosing `TERM` — while `isatty()` (an ioctl) keeps working, which
/// made the gap easy to miss.
///
/// Opened in the parent so the fd is already present in the cloned child: the
/// child path is restricted to raw syscalls on prebuilt pointers, and opening
/// through the peios file API there would mean allocating after clone.
pub(in crate::boundary::linux_launch::process) fn open_console(
    path: &str,
) -> Result<OwnedFd, BoundaryError> {
    let file = OpenOptions::new()
        .desired_access(
            FileAccess::READ_DATA | FileAccess::WRITE_DATA | FileAccess::READ_ATTRIBUTES,
        )
        .open(None, Path::new(path))
        .map_err(|error| BoundaryError::Process(format!("open {path} failed: {error}")))?;
    Ok(file.into())
}
