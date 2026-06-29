mod child;
mod console;
mod report;
mod sys;

use std::ffi::CString;
use std::io;
use std::os::fd::IntoRawFd;

use peios::file::FileAccess;

use crate::boundary::BoundaryError;

use self::child::child_exec_recovery_shell;
use self::console::{CONSOLE_PATH, open_console_fd};
use self::report::{ChildExecReport, read_child_exec_report};
use self::sys::{close_raw_fd, pipe_cloexec};

pub(in crate::init::linux) use self::console::write_console;

pub(super) fn spawn_linux_recovery_shell(path: &str) -> Result<u32, BoundaryError> {
    let shell_path = CString::new(path).map_err(|error| {
        BoundaryError::Recovery(format!("recovery shell path {path} is invalid: {error}"))
    })?;
    let env_path = static_env("PATH=/usr/sbin:/usr/bin:/sbin:/bin")?;
    let env_term = static_env("TERM=linux")?;
    let env_home = static_env("HOME=/root")?;
    let argv = [shell_path.as_ptr(), std::ptr::null()];
    let envp = [
        env_path.as_ptr(),
        env_term.as_ptr(),
        env_home.as_ptr(),
        std::ptr::null(),
    ];

    let console_fd = open_console_fd(FileAccess::READ_DATA | FileAccess::WRITE_DATA)
        .map_err(|error| BoundaryError::Recovery(format!("open {CONSOLE_PATH} failed: {error}")))?
        .into_raw_fd();
    let (error_read_fd, error_write_fd) = pipe_cloexec().map_err(|error| {
        BoundaryError::Recovery(format!("create exec error pipe failed: {error}"))
    })?;

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        close_raw_fd(console_fd);
        close_raw_fd(error_read_fd);
        close_raw_fd(error_write_fd);
        return Err(BoundaryError::Recovery(format!(
            "fork recovery shell {path} failed: {}",
            io::Error::last_os_error()
        )));
    }

    if pid == 0 {
        child_exec_recovery_shell(
            console_fd,
            error_read_fd,
            error_write_fd,
            shell_path.as_ptr(),
            argv.as_ptr(),
            envp.as_ptr(),
        );
    }

    close_raw_fd(console_fd);
    close_raw_fd(error_write_fd);
    let report = read_child_exec_report(error_read_fd);
    close_raw_fd(error_read_fd);
    match report {
        Ok(ChildExecReport::ExecSucceeded) => Ok(pid as u32),
        Ok(ChildExecReport::Failed(failure)) => {
            let _ = wait_for_child_exit(pid as u32);
            Err(BoundaryError::Recovery(format!(
                "exec recovery shell {path} failed at {} with errno {}",
                failure.step.label(),
                failure.errno
            )))
        }
        Err(error) => {
            let _ = wait_for_child_exit(pid as u32);
            Err(BoundaryError::Recovery(format!(
                "read recovery shell exec report failed: {error}"
            )))
        }
    }
}

fn static_env(value: &'static str) -> Result<CString, BoundaryError> {
    CString::new(value).map_err(|error| {
        BoundaryError::Recovery(format!(
            "static recovery environment {value} is invalid: {error}"
        ))
    })
}

pub(super) fn wait_for_child_exit(pid: u32) -> Result<(), BoundaryError> {
    let mut status = 0;
    loop {
        let rc = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, 0) };
        if rc == pid as libc::pid_t {
            return Ok(());
        }
        if rc < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(BoundaryError::Recovery(format!(
                "waitpid recovery shell pid {pid} failed: {error}"
            )));
        }
    }
}
