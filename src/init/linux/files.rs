use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use peios::file::{Disposition, FileAccess, OpenOptions};

use crate::boundary::{BoundaryError, LinuxBootAttemptCounter, read_fd_to_string, write_all_fd};
use crate::init::KernelCommandLine;

use super::mounts::DEFAULT_MOUNTINFO_PATH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LinuxInitFiles {
    command_line_path: PathBuf,
    boot_attempt_counter: LinuxBootAttemptCounter,
    probe_dir: PathBuf,
    mountinfo_path: PathBuf,
}

impl LinuxInitFiles {
    pub const DEFAULT_COMMAND_LINE_PATH: &'static str = "/proc/cmdline";
    pub const DEFAULT_PROBE_DIR: &'static str = "/.peinit";

    pub fn new() -> Self {
        Self {
            command_line_path: PathBuf::from(Self::DEFAULT_COMMAND_LINE_PATH),
            boot_attempt_counter: LinuxBootAttemptCounter::new(),
            probe_dir: PathBuf::from(Self::DEFAULT_PROBE_DIR),
            mountinfo_path: PathBuf::from(DEFAULT_MOUNTINFO_PATH),
        }
    }

    pub fn mountinfo_path(&self) -> &Path {
        &self.mountinfo_path
    }

    pub fn read_kernel_command_line(&mut self) -> Result<KernelCommandLine, BoundaryError> {
        let contents = read_text_file(&self.command_line_path).map_err(|error| {
            BoundaryError::Recovery(format!(
                "read kernel command line {} failed: {error}",
                self.command_line_path.display()
            ))
        })?;
        Ok(KernelCommandLine::parse(&contents))
    }

    pub fn read_boot_attempt_counter(&mut self) -> Result<u32, BoundaryError> {
        self.boot_attempt_counter.read()
    }

    pub fn verify_root_writable(&mut self) -> Result<(), BoundaryError> {
        std::fs::create_dir_all(&self.probe_dir).map_err(|error| {
            BoundaryError::Recovery(format!(
                "create root probe dir {} failed: {error}",
                self.probe_dir.display()
            ))
        })?;
        let probe_path = self.probe_dir.join(format!(
            ".probe-{}-{}",
            std::process::id(),
            monotonic_probe_suffix()
        ));
        write_file(
            &probe_path,
            Disposition::Create,
            b"peinit root write probe\n",
        )
        .map_err(|error| {
            BoundaryError::Recovery(format!(
                "write root probe {} failed: {error}",
                probe_path.display()
            ))
        })?;
        std::fs::remove_file(&probe_path).map_err(|error| {
            BoundaryError::Recovery(format!(
                "remove root probe {} failed: {error}",
                probe_path.display()
            ))
        })
    }

    pub fn increment_boot_attempt_counter(&mut self) -> Result<(), BoundaryError> {
        self.boot_attempt_counter.increment()
    }
}

impl Default for LinuxInitFiles {
    fn default() -> Self {
        Self::new()
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

fn monotonic_probe_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
