use std::io;

pub(super) fn close_fd_checked(fd: i32) -> Result<(), i32> {
    if unsafe { libc::close(fd) } == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}

pub(super) fn dup2_checked(from: i32, to: i32) -> Result<(), i32> {
    loop {
        let result = unsafe { libc::dup2(from, to) };
        if result >= 0 {
            return Ok(());
        }
        let error = errno();
        if error == libc::EINTR {
            continue;
        }
        return Err(error);
    }
}

pub(super) fn clear_cloexec(fd: i32) -> Result<(), i32> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(errno());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}

pub(super) fn write_all_fd(fd: i32, mut bytes: &[u8]) -> Result<(), i32> {
    while !bytes.is_empty() {
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written < 0 {
            let error = errno();
            if error == libc::EINTR {
                continue;
            }
            return Err(error);
        }
        if written == 0 {
            return Err(libc::EIO);
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

pub(super) fn errno() -> i32 {
    io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}
