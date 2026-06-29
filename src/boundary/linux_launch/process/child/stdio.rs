use super::sys::{close_fd_checked, dup2_checked};

pub(super) fn set_standard_streams(
    dev_null_fd: i32,
    stdout_read_fd: i32,
    stdout_write_fd: i32,
    stderr_read_fd: i32,
    stderr_write_fd: i32,
) -> Result<(), i32> {
    close_fd_checked(stdout_read_fd)?;
    close_fd_checked(stderr_read_fd)?;
    dup2_checked(dev_null_fd, libc::STDIN_FILENO)?;
    dup2_checked(stdout_write_fd, libc::STDOUT_FILENO)?;
    dup2_checked(stderr_write_fd, libc::STDERR_FILENO)?;
    if dev_null_fd > libc::STDERR_FILENO {
        close_fd_checked(dev_null_fd)?;
    }
    close_if_not_standard(stdout_write_fd)?;
    close_if_not_standard(stderr_write_fd)?;
    Ok(())
}

fn close_if_not_standard(fd: i32) -> Result<(), i32> {
    if fd <= libc::STDERR_FILENO {
        Ok(())
    } else {
        close_fd_checked(fd)
    }
}

/// Console-attached variant: point stdin/stdout/stderr at a live console tty
/// instead of `/dev/null` + capture pipes. Used for the compiled-in console
/// service so its shell reads keystrokes and writes to the screen. The daemon
/// fds (dev-null + both pipe ends) are unused here and closed so nothing leaks
/// across the exec and the parent's pipe readers see EOF promptly.
pub(super) fn set_console_streams(
    console_fd: i32,
    dev_null_fd: i32,
    stdout_read_fd: i32,
    stdout_write_fd: i32,
    stderr_read_fd: i32,
    stderr_write_fd: i32,
) -> Result<(), i32> {
    for fd in [
        dev_null_fd,
        stdout_read_fd,
        stdout_write_fd,
        stderr_read_fd,
        stderr_write_fd,
    ] {
        if fd > libc::STDERR_FILENO {
            close_fd_checked(fd)?;
        }
    }
    dup2_checked(console_fd, libc::STDIN_FILENO)?;
    dup2_checked(console_fd, libc::STDOUT_FILENO)?;
    dup2_checked(console_fd, libc::STDERR_FILENO)?;
    if console_fd > libc::STDERR_FILENO {
        close_fd_checked(console_fd)?;
    }
    Ok(())
}
