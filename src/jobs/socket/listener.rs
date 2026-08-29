use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use crate::control::socket::address::{unix_socket_address, unlink_path, unlink_stale_path};
use crate::control::socket::is_would_block;

use super::connection::LinuxJobsConnection;
use super::model::{JOBS_SOCKET_LISTEN_BACKLOG, JobsSocketAcceptError, JobsSocketBindError};

/// The listening end of the jobs channel: `SOCK_SEQPACKET`, close-on-exec,
/// non-blocking, unlinked when dropped.
#[derive(Debug)]
pub struct LinuxJobsSocket {
    fd: OwnedFd,
    path: PathBuf,
}

impl LinuxJobsSocket {
    pub fn bind(path: impl AsRef<Path>) -> Result<Self, JobsSocketBindError> {
        let path = path.as_ref().to_path_buf();
        let address = unix_socket_address(&path).map_err(JobsSocketBindError::Path)?;
        unlink_stale_path(&path)?;

        let fd = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                0,
            )
        };
        if fd < 0 {
            return Err(JobsSocketBindError::Socket(io::Error::last_os_error()));
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
            return Err(JobsSocketBindError::Bind {
                path,
                source: io::Error::last_os_error(),
            });
        }
        let listen_rc = unsafe { libc::listen(fd.as_raw_fd(), JOBS_SOCKET_LISTEN_BACKLOG) };
        if listen_rc < 0 {
            return Err(JobsSocketBindError::Listen(io::Error::last_os_error()));
        }
        Ok(Self { fd, path })
    }

    pub fn as_raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn accept(&self) -> Result<Option<LinuxJobsConnection>, JobsSocketAcceptError> {
        let fd = unsafe {
            libc::accept4(
                self.fd.as_raw_fd(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            )
        };
        if fd >= 0 {
            return Ok(Some(LinuxJobsConnection::from_owned(unsafe {
                OwnedFd::from_raw_fd(fd)
            })));
        }
        let error = io::Error::last_os_error();
        if is_would_block(&error) {
            Ok(None)
        } else {
            Err(JobsSocketAcceptError::Accept(error))
        }
    }
}

impl Drop for LinuxJobsSocket {
    fn drop(&mut self) {
        let _ = unlink_path(&self.path);
    }
}
