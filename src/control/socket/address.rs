use std::ffi::CString;
use std::io;
use std::mem::{MaybeUninit, size_of};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::{ControlSocketBindError, ControlSocketPathError};

#[derive(Debug, Clone, Copy)]
pub(super) struct UnixSocketAddress {
    pub(super) addr: libc::sockaddr_un,
    pub(super) len: libc::socklen_t,
}

pub(super) fn unix_socket_address(
    path: &Path,
) -> Result<UnixSocketAddress, ControlSocketPathError> {
    let path_bytes = path.as_os_str().as_bytes();
    if path_bytes.is_empty() {
        return Err(ControlSocketPathError::Empty);
    }

    let mut addr = unsafe { MaybeUninit::<libc::sockaddr_un>::zeroed().assume_init() };
    let max_path = addr.sun_path.len().saturating_sub(1);
    if path_bytes.len() > max_path {
        return Err(ControlSocketPathError::TooLong {
            len: path_bytes.len(),
            max: max_path,
        });
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (index, byte) in path_bytes.iter().copied().enumerate() {
        if byte == 0 {
            return Err(ControlSocketPathError::InteriorNul { index });
        }
        addr.sun_path[index] = byte as libc::c_char;
    }

    Ok(UnixSocketAddress {
        addr,
        len: (size_of::<libc::sa_family_t>() + path_bytes.len() + 1) as libc::socklen_t,
    })
}

pub(super) fn unlink_stale_path(path: &Path) -> Result<(), ControlSocketBindError> {
    match unlink_path(path) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => Ok(()),
        Err(source) => Err(ControlSocketBindError::StalePathCleanup {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn unlink_path(path: &Path) -> io::Result<()> {
    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("interior NUL at byte {}", error.nul_position()),
        )
    })?;
    let rc = unsafe { libc::unlink(c_path.as_ptr()) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
