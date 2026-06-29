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
