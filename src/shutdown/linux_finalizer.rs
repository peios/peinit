use std::ffi::CString;
#[cfg(not(feature = "peios-boundary"))]
use std::fs;
use std::io;
#[cfg(feature = "peios-boundary")]
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;

#[cfg(feature = "peios-boundary")]
use peios::file::{FileAccess, OpenOptions};

#[cfg(feature = "peios-boundary")]
use crate::boundary::read_fd_to_string;
use crate::boundary::{BoundaryError, ShutdownFinalizer, save_linux_random_seed};

use super::ShutdownKind;
use super::mountinfo::{mountinfo_contains_mount_point, parse_mountinfo_mount_points};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxShutdownFinalizer {
    mountinfo_path: PathBuf,
}

impl LinuxShutdownFinalizer {
    pub const DEFAULT_MOUNTINFO_PATH: &'static str = "/proc/self/mountinfo";

    pub fn new() -> Self {
        Self {
            mountinfo_path: PathBuf::from(Self::DEFAULT_MOUNTINFO_PATH),
        }
    }

    pub fn with_mountinfo_path(path: impl Into<PathBuf>) -> Self {
        Self {
            mountinfo_path: path.into(),
        }
    }

    fn read_mountinfo(&self) -> Result<String, BoundaryError> {
        read_text_file(&self.mountinfo_path).map_err(|error| {
            BoundaryError::Shutdown(format!(
                "read {} failed: {error}",
                self.mountinfo_path.display()
            ))
        })
    }

    fn is_still_mounted(&self, mount_point: &str) -> Result<bool, BoundaryError> {
        mountinfo_contains_mount_point(&self.read_mountinfo()?, mount_point)
    }
}

impl Default for LinuxShutdownFinalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "peios-boundary"))]
fn read_text_file(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

#[cfg(feature = "peios-boundary")]
fn read_text_file(path: &Path) -> io::Result<String> {
    let file = OpenOptions::new()
        .desired_access(FileAccess::READ_DATA)
        .open(None, path)
        .map_err(io::Error::other)?;
    read_fd_to_string(file.as_raw_fd())
}

impl ShutdownFinalizer for LinuxShutdownFinalizer {
    fn save_random_seed(&mut self) -> Result<(), BoundaryError> {
        save_linux_random_seed()
            .map_err(|error| BoundaryError::Shutdown(format!("save random seed failed: {error:?}")))
    }

    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        parse_mountinfo_mount_points(&self.read_mountinfo()?)
    }

    fn unmount(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        let target = c_string_path(mount_point)?;
        let result = unsafe { libc::umount2(target.as_ptr(), 0) };
        if result == 0 {
            return Ok(());
        }

        let error = io::Error::last_os_error();
        if already_unmounted_errno(&error) && !self.is_still_mounted(mount_point)? {
            return Ok(());
        }
        Err(syscall_error("umount2", mount_point, error))
    }

    fn remount_readonly(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        let target = c_string_path(mount_point)?;
        let flags = libc::MS_REMOUNT | libc::MS_RDONLY;
        let result = unsafe {
            libc::mount(
                std::ptr::null(),
                target.as_ptr(),
                std::ptr::null(),
                flags,
                std::ptr::null(),
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(syscall_error(
                "mount(MS_REMOUNT|MS_RDONLY)",
                mount_point,
                io::Error::last_os_error(),
            ))
        }
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        unsafe { libc::sync() };
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        let command = reboot_command(kind);
        let result = unsafe { libc::reboot(command) };
        if result == 0 {
            Ok(())
        } else {
            Err(syscall_error(
                "reboot",
                kind.name(),
                io::Error::last_os_error(),
            ))
        }
    }
}

fn c_string_path(path: &str) -> Result<CString, BoundaryError> {
    CString::new(path).map_err(|_| BoundaryError::Shutdown(format!("path contains NUL: {path:?}")))
}

fn already_unmounted_errno(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(errno) if errno == libc::ENOENT || errno == libc::EINVAL
    )
}

fn reboot_command(kind: ShutdownKind) -> libc::c_int {
    match kind {
        ShutdownKind::Poweroff => libc::RB_POWER_OFF,
        ShutdownKind::Reboot => libc::RB_AUTOBOOT,
        ShutdownKind::Halt => libc::RB_HALT_SYSTEM,
    }
}

fn syscall_error(syscall: &str, target: impl AsRef<str>, error: io::Error) -> BoundaryError {
    BoundaryError::Shutdown(format!("{syscall}({}) failed: {error}", target.as_ref()))
}

impl ShutdownKind {
    fn name(self) -> &'static str {
        match self {
            Self::Poweroff => "poweroff",
            Self::Reboot => "reboot",
            Self::Halt => "halt",
        }
    }
}

#[cfg(all(test, not(feature = "peios-boundary")))]
mod tests;
