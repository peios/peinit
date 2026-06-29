use super::sys::{close_fd_checked, errno, write_all_fd};

pub(super) fn set_rlimits(limit_nofile: Option<u64>, limit_core: Option<u64>) -> Result<(), i32> {
    if let Some(limit) = limit_nofile {
        set_rlimit(libc::RLIMIT_NOFILE, limit)?;
    }
    if let Some(limit) = limit_core {
        set_rlimit(libc::RLIMIT_CORE, limit)?;
    }
    Ok(())
}

pub(super) fn set_oom_score_adj(value: i32) -> Result<(), i32> {
    let bytes = match value {
        -1000 => b"-1000\n".as_slice(),
        0 => b"0\n".as_slice(),
        _ => return Err(libc::EINVAL),
    };
    let path = b"/proc/self/oom_score_adj\0";
    let fd = unsafe { libc::open(path.as_ptr().cast(), libc::O_WRONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(errno());
    }
    let result = write_all_fd(fd, bytes);
    let close_result = close_fd_checked(fd);
    result?;
    close_result
}

pub(super) fn change_working_directory(path: *const libc::c_char) -> Result<(), i32> {
    if unsafe { libc::chdir(path) } == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}

fn set_rlimit(resource: libc::__rlimit_resource_t, limit: u64) -> Result<(), i32> {
    let rlim = limit as libc::rlim_t;
    if rlim as u64 != limit {
        return Err(libc::EINVAL);
    }
    let value = libc::rlimit {
        rlim_cur: rlim,
        rlim_max: rlim,
    };
    if unsafe { libc::setrlimit(resource, &value) } == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}
