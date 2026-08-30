mod receive;

#[cfg(test)]
mod tests;

use std::io;
use std::mem::size_of;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};

use receive::receive_datagram;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyCredentials {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug)]
pub struct NotifyDatagram {
    pub payload: Vec<u8>,
    pub credentials: NotifyCredentials,
    pub fds: Vec<OwnedFd>,
}

#[derive(Debug)]
pub struct NotifySocket {
    socket: UnixDatagram,
    path: PathBuf,
}

#[derive(Debug)]
pub enum NotifySocketBindError {
    StalePathCleanup { path: PathBuf, source: io::Error },
    Bind { path: PathBuf, source: io::Error },
    SetPassCred(io::Error),
    SetNonblocking(io::Error),
    /// The socket bound but its security descriptor could not be installed.
    ///
    /// Fatal rather than a warning: the socket exists at that point, reachable
    /// under whatever it inherited, and continuing would serve it under a
    /// descriptor nobody chose.
    Secure(io::Error),
}

#[derive(Debug)]
pub enum NotifySocketReadError {
    WouldBlock,
    Recv(io::Error),
    MissingCredentials,
    /// The kernel did not deliver the datagram whole.
    ///
    /// Not a socket failure: the datagram was consumed and is gone. It is a
    /// rejection of that one message, and the runtime records it the way it
    /// records a malformed line.
    Truncated {
        payload: bool,
        control: bool,
    },
}

impl NotifySocket {
    pub fn bind(path: impl AsRef<Path>) -> Result<Self, NotifySocketBindError> {
        Self::bind_secured(path, |_| Ok(()))
    }

    /// Bind, and stamp a security descriptor onto the socket before it is used.
    ///
    /// `secure` runs between `bind` and everything else, which is the point.
    /// `bind` publishes a pathname socket under whatever descriptor it inherits
    /// from the parent directory, and a datagram socket has no `listen` to hold
    /// it back — so a caller stamping afterwards, by path, leaves a window in
    /// which the socket is reachable under the wrong descriptor. Passing the
    /// stamp in closes it.
    ///
    /// The fd is borrowed rather than handed over: this socket is about to
    /// serve, and `peios::file::File` would close it on drop.
    pub fn bind_secured(
        path: impl AsRef<Path>,
        secure: impl FnOnce(BorrowedFd<'_>) -> io::Result<()>,
    ) -> Result<Self, NotifySocketBindError> {
        let path = path.as_ref().to_path_buf();
        unlink_stale_path(&path)?;
        let socket = UnixDatagram::bind(&path).map_err(|source| NotifySocketBindError::Bind {
            path: path.clone(),
            source,
        })?;
        secure(socket.as_fd()).map_err(NotifySocketBindError::Secure)?;
        set_passcred(socket.as_raw_fd()).map_err(NotifySocketBindError::SetPassCred)?;
        socket
            .set_nonblocking(true)
            .map_err(NotifySocketBindError::SetNonblocking)?;
        Ok(Self { socket, path })
    }

    pub fn as_raw_fd(&self) -> i32 {
        self.socket.as_raw_fd()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn receive(&self) -> Result<NotifyDatagram, NotifySocketReadError> {
        receive_datagram(self.socket.as_raw_fd())
    }
}

impl Drop for NotifySocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn set_passcred(fd: RawFd) -> io::Result<()> {
    let enabled: libc::c_int = 1;
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            &enabled as *const _ as *const libc::c_void,
            size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn unlink_stale_path(path: &Path) -> Result<(), NotifySocketBindError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => Ok(()),
        Err(source) => Err(NotifySocketBindError::StalePathCleanup {
            path: path.to_path_buf(),
            source,
        }),
    }
}
