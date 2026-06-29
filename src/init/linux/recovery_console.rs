use std::io;
use std::path::Path;

use peios::file::{FileAccess, OpenOptions};

use crate::boundary::BoundaryError;
use crate::init::InitRecoveryReason;

mod process;

pub(super) use process::write_console;
use process::{spawn_linux_recovery_shell, wait_for_child_exit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RecoveryShell {
    Recsh,
    Sh,
}

impl RecoveryShell {
    fn path(self) -> &'static str {
        match self {
            Self::Recsh => "/bin/recsh",
            Self::Sh => "/bin/sh",
        }
    }
}

pub(super) fn run_recovery_console_forever<B>(
    boundary: &mut B,
    reason: &InitRecoveryReason,
) -> Result<(), BoundaryError>
where
    B: RecoveryConsoleBoundary + ?Sized,
{
    run_recovery_console(boundary, reason, RecoveryLoopLimit::Forever)
}

fn run_recovery_console<B>(
    boundary: &mut B,
    reason: &InitRecoveryReason,
    limit: RecoveryLoopLimit,
) -> Result<(), BoundaryError>
where
    B: RecoveryConsoleBoundary + ?Sized,
{
    let _ = boundary.log_console(&format!("peinit entering Recovery mode: {reason:?}\n"));
    let mut completed_sessions = 0usize;

    loop {
        if limit.reached(completed_sessions) {
            return Ok(());
        }

        let Some(shell) = select_recovery_shell(boundary)? else {
            return halt_without_recovery_shell(
                boundary,
                "neither /bin/recsh nor /bin/sh is executable",
            );
        };

        match spawn_selected_shell(boundary, shell) {
            Ok(pid) => {
                boundary.wait_for_shell(pid)?;
                completed_sessions += 1;
            }
            Err(error) => {
                return halt_without_recovery_shell(
                    boundary,
                    &format!("failed to start recovery shell: {error:?}"),
                );
            }
        }
    }
}

fn spawn_selected_shell<B>(boundary: &mut B, shell: RecoveryShell) -> Result<u32, BoundaryError>
where
    B: RecoveryConsoleBoundary + ?Sized,
{
    match boundary.spawn_shell(shell) {
        Ok(pid) => Ok(pid),
        Err(first_error) if shell == RecoveryShell::Recsh => {
            if boundary.can_execute(RecoveryShell::Sh)? {
                boundary.spawn_shell(RecoveryShell::Sh)
            } else {
                Err(first_error)
            }
        }
        Err(error) => Err(error),
    }
}

fn select_recovery_shell<B>(boundary: &mut B) -> Result<Option<RecoveryShell>, BoundaryError>
where
    B: RecoveryConsoleBoundary + ?Sized,
{
    for shell in [RecoveryShell::Recsh, RecoveryShell::Sh] {
        if boundary.can_execute(shell)? {
            return Ok(Some(shell));
        }
    }
    Ok(None)
}

fn halt_without_recovery_shell<B>(boundary: &mut B, message: &str) -> Result<(), BoundaryError>
where
    B: RecoveryConsoleBoundary + ?Sized,
{
    let _ = boundary.log_console(&format!("{message}\n"));
    let _ = boundary.sync_filesystems();
    boundary.halt()?;
    Err(BoundaryError::Recovery(
        "halt returned after recovery shell failure".to_string(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryLoopLimit {
    Forever,
    #[cfg(test)]
    Sessions(usize),
}

impl RecoveryLoopLimit {
    fn reached(self, completed_sessions: usize) -> bool {
        #[cfg(not(test))]
        let _ = completed_sessions;
        match self {
            Self::Forever => false,
            #[cfg(test)]
            Self::Sessions(max) => completed_sessions >= max,
        }
    }
}

pub(super) trait RecoveryConsoleBoundary {
    fn can_execute(&mut self, shell: RecoveryShell) -> Result<bool, BoundaryError>;
    fn spawn_shell(&mut self, shell: RecoveryShell) -> Result<u32, BoundaryError>;
    fn wait_for_shell(&mut self, pid: u32) -> Result<(), BoundaryError>;
    fn log_console(&mut self, message: &str) -> Result<(), BoundaryError>;
    fn sync_filesystems(&mut self) -> Result<(), BoundaryError>;
    fn halt(&mut self) -> Result<(), BoundaryError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LinuxRecoveryConsoleBoundary;

impl RecoveryConsoleBoundary for LinuxRecoveryConsoleBoundary {
    fn can_execute(&mut self, shell: RecoveryShell) -> Result<bool, BoundaryError> {
        match OpenOptions::new()
            .desired_access(FileAccess::EXECUTE)
            .open(None, Path::new(shell.path()))
        {
            Ok(_) => Ok(true),
            Err(error) if executable_absent_or_denied(&error) => Ok(false),
            Err(error) => Err(BoundaryError::Recovery(format!(
                "probe executable {} failed: {error}",
                shell.path()
            ))),
        }
    }

    fn spawn_shell(&mut self, shell: RecoveryShell) -> Result<u32, BoundaryError> {
        spawn_linux_recovery_shell(shell.path())
    }

    fn wait_for_shell(&mut self, pid: u32) -> Result<(), BoundaryError> {
        wait_for_child_exit(pid)
    }

    fn log_console(&mut self, message: &str) -> Result<(), BoundaryError> {
        write_console(message)
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        unsafe { libc::sync() };
        Ok(())
    }

    fn halt(&mut self) -> Result<(), BoundaryError> {
        let rc = unsafe { libc::reboot(libc::LINUX_REBOOT_CMD_HALT) };
        if rc < 0 {
            Err(BoundaryError::Recovery(format!(
                "halt after recovery shell failure failed: {}",
                io::Error::last_os_error()
            )))
        } else {
            Ok(())
        }
    }
}

fn executable_absent_or_denied(error: &peios::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(errno) if errno == libc::ENOENT || errno == libc::ENOTDIR || errno == libc::EACCES
    )
}

#[cfg(test)]
mod tests;
