use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;

use peios::file::{FileAccess, OpenOptions};

use crate::boundary::{BoundaryError, ConsoleSink, write_all_fd};

pub const CONSOLE_PATH: &str = "/dev/console";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinuxConsoleSink;

impl LinuxConsoleSink {
    pub fn new() -> Self {
        Self
    }
}

impl ConsoleSink for LinuxConsoleSink {
    fn write_console(&mut self, message: &str) -> Result<(), BoundaryError> {
        write_linux_console_message(message)
    }
}

pub fn open_linux_console_fd(access: FileAccess) -> io::Result<OwnedFd> {
    let file = OpenOptions::new()
        .desired_access(access)
        .open(None, Path::new(CONSOLE_PATH))
        .map_err(io::Error::from)?;
    Ok(file.into())
}

pub fn write_linux_console_message(message: &str) -> Result<(), BoundaryError> {
    let fd = open_linux_console_fd(FileAccess::WRITE_DATA)
        .map_err(|error| BoundaryError::Recovery(format!("open {CONSOLE_PATH} failed: {error}")))?;
    write_all_fd(fd.as_raw_fd(), message.as_bytes()).map_err(|error| {
        BoundaryError::Recovery(format!(
            "write console message to {CONSOLE_PATH} failed: {error}"
        ))
    })
}
