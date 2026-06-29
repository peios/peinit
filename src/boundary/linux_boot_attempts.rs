use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use peios::file::{Disposition, FileAccess, OpenOptions};

use crate::boundary::{BootAttemptCounter, BoundaryError, read_fd_to_string, write_all_fd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxBootAttemptCounter {
    path: PathBuf,
}

impl LinuxBootAttemptCounter {
    pub const DEFAULT_BOOT_ATTEMPTS_PATH: &'static str = "/.peinit/boot-attempts";

    pub fn new() -> Self {
        Self {
            path: PathBuf::from(Self::DEFAULT_BOOT_ATTEMPTS_PATH),
        }
    }

    pub fn read(&mut self) -> Result<u32, BoundaryError> {
        match read_text_file(&self.path) {
            Ok(contents) => parse_boot_attempt_counter(&contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(BoundaryError::Recovery(format!(
                "read boot-attempt counter {} failed: {error}",
                self.path.display()
            ))),
        }
    }

    pub fn increment(&mut self) -> Result<(), BoundaryError> {
        let current = self.read().unwrap_or(0);
        self.write(current.saturating_add(1))
    }

    pub fn reset(&mut self) -> Result<(), BoundaryError> {
        self.write(0)
    }

    fn write(&mut self, value: u32) -> Result<(), BoundaryError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                BoundaryError::Recovery(format!(
                    "create boot-attempt counter dir {} failed: {error}",
                    parent.display()
                ))
            })?;
        }
        write_file(
            &self.path,
            Disposition::OverwriteIf,
            format!("{value}\n").as_bytes(),
        )
        .map_err(|error| {
            BoundaryError::Recovery(format!(
                "write boot-attempt counter {} failed: {error}",
                self.path.display()
            ))
        })
    }
}

impl Default for LinuxBootAttemptCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl BootAttemptCounter for LinuxBootAttemptCounter {
    fn reset_boot_attempt_counter(&mut self) -> Result<(), BoundaryError> {
        self.reset()
    }
}

fn read_text_file(path: &Path) -> io::Result<String> {
    let file = OpenOptions::new()
        .desired_access(FileAccess::READ_DATA)
        .open(None, path)
        .map_err(io::Error::from)?;
    read_fd_to_string(file.as_raw_fd())
}

fn write_file(path: &Path, disposition: Disposition, bytes: &[u8]) -> io::Result<()> {
    let file = OpenOptions::new()
        .desired_access(FileAccess::WRITE_DATA)
        .disposition(disposition)
        .open(None, path)
        .map_err(io::Error::from)?;
    write_all_fd(file.as_raw_fd(), bytes)
}

fn parse_boot_attempt_counter(contents: &str) -> Result<u32, BoundaryError> {
    let trimmed = contents.trim_end_matches(char::is_whitespace);
    if trimmed.is_empty() {
        return Err(BoundaryError::Recovery(
            "boot-attempt counter is empty".to_string(),
        ));
    }
    trimmed.parse::<u32>().map_err(|error| {
        BoundaryError::Recovery(format!(
            "boot-attempt counter is not a valid integer: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::parse_boot_attempt_counter;

    #[test]
    fn parses_boot_attempt_counter_with_trailing_whitespace() {
        assert_eq!(parse_boot_attempt_counter("42\n").expect("counter"), 42);
    }

    #[test]
    fn rejects_empty_boot_attempt_counter() {
        assert!(parse_boot_attempt_counter("\n").is_err());
    }
}
