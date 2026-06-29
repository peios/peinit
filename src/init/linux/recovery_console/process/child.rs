use std::ffi::c_char;

use super::report::{ChildFailureStep, write_child_failure};
use super::sys::{close_raw_fd, last_errno};

pub(super) fn child_exec_recovery_shell(
    console_fd: i32,
    error_read_fd: i32,
    error_write_fd: i32,
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> ! {
    close_raw_fd(error_read_fd);
    for target_fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        if unsafe { libc::dup2(console_fd, target_fd) } < 0 {
            write_child_failure(error_write_fd, ChildFailureStep::DupConsole, last_errno());
        }
    }
    if console_fd > libc::STDERR_FILENO {
        close_raw_fd(console_fd);
    }
    unsafe { libc::execve(path, argv, envp) };
    write_child_failure(error_write_fd, ChildFailureStep::Exec, last_errno());
}
