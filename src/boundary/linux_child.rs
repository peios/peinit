use std::io;

use super::{BoundaryError, ChildExitStatus, ChildReap, ChildReaper};

const WAIT_ANY_CHILD: i32 = -1;
const WAIT_NONBLOCK_FLAGS: i32 = libc::WNOHANG;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinuxChildReaper;

impl LinuxChildReaper {
    pub fn new() -> Self {
        Self
    }
}

impl ChildReaper for LinuxChildReaper {
    fn reap_children(&mut self) -> Result<Vec<ChildReap>, BoundaryError> {
        let mut syscalls = LinuxChildReapSyscalls;
        drain_linux_child_reaps(&mut syscalls)
            .map_err(|error| BoundaryError::Process(format!("{error:?}")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxWaitPid {
    Reaped { pid: i64, status: i32 },
    NoStatus,
    NoChildren,
}

#[derive(Debug)]
pub enum LinuxChildReapError {
    Wait { source: io::Error },
    PidOutOfRange { returned_pid: i64 },
    UnsupportedStatus { pid: u32, status: i32 },
}

pub fn drain_linux_child_reaps<S>(syscalls: &mut S) -> Result<Vec<ChildReap>, LinuxChildReapError>
where
    S: LinuxChildReapSyscallApi + ?Sized,
{
    let mut reaped = Vec::new();
    loop {
        match wait_linux_child(syscalls)? {
            Some(child) => reaped.push(child),
            None => return Ok(reaped),
        }
    }
}

pub fn wait_linux_child<S>(syscalls: &mut S) -> Result<Option<ChildReap>, LinuxChildReapError>
where
    S: LinuxChildReapSyscallApi + ?Sized,
{
    match syscalls
        .waitpid(WAIT_ANY_CHILD, WAIT_NONBLOCK_FLAGS)
        .map_err(|source| LinuxChildReapError::Wait { source })?
    {
        LinuxWaitPid::NoStatus | LinuxWaitPid::NoChildren => Ok(None),
        LinuxWaitPid::Reaped { pid, status } => {
            let pid = u32::try_from(pid)
                .map_err(|_| LinuxChildReapError::PidOutOfRange { returned_pid: pid })?;
            Ok(Some(ChildReap {
                pid,
                status: normalize_linux_wait_status(pid, status)?,
            }))
        }
    }
}

pub fn normalize_linux_wait_status(
    pid: u32,
    status: i32,
) -> Result<ChildExitStatus, LinuxChildReapError> {
    if libc::WIFEXITED(status) {
        return Ok(ChildExitStatus::Exited {
            code: libc::WEXITSTATUS(status),
        });
    }
    if libc::WIFSIGNALED(status) {
        return Ok(ChildExitStatus::Signaled {
            signal: libc::WTERMSIG(status),
            core_dumped: libc::WCOREDUMP(status),
        });
    }
    Err(LinuxChildReapError::UnsupportedStatus { pid, status })
}

pub trait LinuxChildReapSyscallApi {
    fn waitpid(&mut self, pid: i32, options: i32) -> io::Result<LinuxWaitPid>;
}

#[derive(Debug, Clone, Copy, Default)]
struct LinuxChildReapSyscalls;

impl LinuxChildReapSyscallApi for LinuxChildReapSyscalls {
    fn waitpid(&mut self, pid: i32, options: i32) -> io::Result<LinuxWaitPid> {
        let mut status = 0;
        let rc = unsafe { libc::waitpid(pid, &mut status, options) };
        if rc > 0 {
            return Ok(LinuxWaitPid::Reaped {
                pid: i64::from(rc),
                status,
            });
        }
        if rc == 0 {
            return Ok(LinuxWaitPid::NoStatus);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ECHILD) {
            Ok(LinuxWaitPid::NoChildren)
        } else {
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
