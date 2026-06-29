use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use super::address::{unix_socket_address, unlink_path, unlink_stale_path};
use super::connection::{LinuxControlConnection, is_would_block};
use super::{CONTROL_SOCKET_LISTEN_BACKLOG, ControlSocketAcceptError, ControlSocketBindError};

#[derive(Debug)]
pub struct LinuxControlSocket {
    fd: OwnedFd,
    path: PathBuf,
}

impl LinuxControlSocket {
    pub fn bind(path: impl AsRef<Path>) -> Result<Self, ControlSocketBindError> {
        let path = path.as_ref().to_path_buf();
        let address = unix_socket_address(&path).map_err(ControlSocketBindError::Path)?;
        unlink_stale_path(&path)?;

        let fd = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                0,
            )
        };
        if fd < 0 {
            return Err(ControlSocketBindError::Socket(io::Error::last_os_error()));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };

        let bind_rc = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                (&address.addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
                address.len,
            )
        };
        if bind_rc < 0 {
            return Err(ControlSocketBindError::Bind {
                path,
                source: io::Error::last_os_error(),
            });
        }

        let listen_rc = unsafe { libc::listen(fd.as_raw_fd(), CONTROL_SOCKET_LISTEN_BACKLOG) };
        if listen_rc < 0 {
            return Err(ControlSocketBindError::Listen(io::Error::last_os_error()));
        }

        Ok(Self { fd, path })
    }

    pub fn as_raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn accept(&self) -> Result<Option<LinuxControlConnection>, ControlSocketAcceptError> {
        match accept_connection(self.fd.as_raw_fd())? {
            Some(fd) => Ok(Some(LinuxControlConnection { fd })),
            None => Ok(None),
        }
    }
}

impl Drop for LinuxControlSocket {
    fn drop(&mut self) {
        let _ = unlink_path(&self.path);
    }
}

fn accept_connection(listener_fd: i32) -> Result<Option<OwnedFd>, ControlSocketAcceptError> {
    let fd = unsafe {
        libc::accept4(
            listener_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
        )
    };
    if fd >= 0 {
        return Ok(Some(unsafe { OwnedFd::from_raw_fd(fd) }));
    }

    let error = io::Error::last_os_error();
    if is_would_block(&error) {
        Ok(None)
    } else {
        Err(ControlSocketAcceptError::Accept(error))
    }
}
