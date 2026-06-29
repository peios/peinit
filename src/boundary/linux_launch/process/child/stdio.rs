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
