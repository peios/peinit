use std::io;

pub(super) fn is_would_block(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK
    )
}

pub(super) fn short_read_error(source: &str, read: usize, expected: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!("short {source} read: {read} of {expected} bytes"),
    )
}

#[cfg(feature = "peios-boundary")]
pub(crate) fn read_fd_to_string(fd: i32) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read as usize]);
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(feature = "peios-boundary")]
pub(crate) fn write_all_fd(fd: i32, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "write returned zero bytes",
            ));
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

/// Set `FD_CLOEXEC` on a descriptor the Peios native open returned.
///
/// `kacs_native_open` calls `get_unused_fd_flags(0)` and `struct kacs_open_how`
/// carries no CLOEXEC bit, so every `peios::file` open hands back a descriptor
/// that survives exec. peinit TRM §5.4 requires the opposite of everything it
/// holds, so each such fd it keeps past a launch has to be repaired by hand.
#[cfg(feature = "peios-boundary")]
pub(crate) fn set_cloexec(fd: i32) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(test, feature = "peios-boundary"))]
mod cloexec_tests {
    use super::set_cloexec;

    #[test]
    fn set_cloexec_sets_the_bit_on_a_descriptor_that_lacks_it() {
        // pipe2(0) is what the native open effectively gives us: a valid fd
        // with FD_CLOEXEC clear.
        let mut fds = [0_i32; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), 0) }, 0);
        let fd = fds[0];

        let before = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(before >= 0);
        assert_eq!(
            before & libc::FD_CLOEXEC,
            0,
            "fixture must start without CLOEXEC or the test proves nothing"
        );

        set_cloexec(fd).expect("set_cloexec");

        let after = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(after >= 0);
        assert_eq!(after & libc::FD_CLOEXEC, libc::FD_CLOEXEC);

        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }

    #[test]
    fn set_cloexec_reports_a_bad_descriptor() {
        let error = set_cloexec(-1).expect_err("a closed fd must not succeed");
        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
    }
}
