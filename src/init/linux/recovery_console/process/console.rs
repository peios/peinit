use std::io;
use std::os::fd::OwnedFd;

use peios::file::FileAccess;

use crate::boundary::{BoundaryError, open_linux_console_fd, write_linux_console_message};

pub(super) use crate::boundary::CONSOLE_PATH;

pub(super) fn open_console_fd(access: FileAccess) -> io::Result<OwnedFd> {
    open_linux_console_fd(access)
}

pub(in crate::init::linux) fn write_console(message: &str) -> Result<(), BoundaryError> {
    write_linux_console_message(message)
}
