use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use peios::file::{FileAccess, OpenOptions};

use crate::boundary::read_fd_to_string;

pub(super) fn pidfd_send_signal(pidfd: i32, signal: libc::c_int) -> io::Result<()> {
    if pidfd < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid pidfd {pidfd}"),
        ));
    }

    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd,
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn pidfd_matches_pid(pidfd: i32, pid: u32) -> io::Result<bool> {
    if pidfd < 0 {
        return Ok(false);
    }
    let Some(fdinfo_pid) = pid_from_pidfd_fdinfo(pidfd)? else {
        return Ok(false);
    };
    if fdinfo_pid != pid {
        return Ok(false);
    }
    match pidfd_send_signal(pidfd, 0) {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(false),
        Err(error) => Err(error),
    }
}

fn pid_from_pidfd_fdinfo(pidfd: i32) -> io::Result<Option<u32>> {
    let path = Path::new("/proc/self/fdinfo").join(pidfd.to_string());
    let file = OpenOptions::new()
        .desired_access(FileAccess::READ_DATA)
        .open(None, &path)
        .map_err(io::Error::other)?;
    let text = read_fd_to_string(file.as_raw_fd())?;
    Ok(parse_fdinfo_pid(&text))
}

fn parse_fdinfo_pid(text: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix("Pid:")?.trim();
        let pid = value.parse::<i64>().ok()?;
        u32::try_from(pid).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::parse_fdinfo_pid;

    #[test]
    fn parses_pidfd_fdinfo_pid_field() {
        assert_eq!(
            parse_fdinfo_pid("pos:\t0\nflags:\t02000002\nPid:\t1234\nNSpid:\t1234\n"),
            Some(1234),
        );
        assert_eq!(parse_fdinfo_pid("Pid:\t-1\n"), None);
        assert_eq!(parse_fdinfo_pid("flags:\t02000002\n"), None);
    }
}
