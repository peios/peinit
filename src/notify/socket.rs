mod receive;

#[cfg(test)]
mod tests;

use std::io;
use std::mem::size_of;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
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
    StalePathCleanup {
        path: PathBuf,
        source: io::Error,
    },
    Bind {
        path: PathBuf,
        source: io::Error,
    },
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

/// The descriptor every peinit notification socket carries.
///
/// `S-1-5-6` is the Service group, which every token minted for a service logon
/// carries. It is the right grantee because it *is* the population with
/// something to say here: membership follows from having been started as a
/// service rather than from which account a process runs under, so an ordinary
/// user process cannot acquire it however it was launched.
///
/// `FW` is the file-write generic right, which is what a write to a pathname
/// socket needs — the same reasoning as the jobs socket. Administrators are
/// deliberately absent, unlike the control socket: an administrator has no
/// business asserting that a service is ready.
///
/// The SID is written out rather than as its `SU` alias, because this is on the
/// boot path and the alias table lives in a separately versioned libpeios that
/// nothing here declares a minimum version of.
///
/// **One constant, and it lives here rather than at a call site**, because
/// peinit binds this socket twice: once in Phase 1 for registryd, and again
/// when the main runtime starts. `bind` unlinks a stale path, so the second
/// bind replaces the first — and a descriptor applied at only one of them is a
/// descriptor the running system does not have.
pub const NOTIFY_SOCKET_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;FW;;;S-1-5-6)";

impl NotifySocket {
    pub fn bind(path: impl AsRef<Path>) -> Result<Self, NotifySocketBindError> {
        Self::bind_secured(path, |_| Ok(()))
    }

    /// Bind, and stamp a security descriptor onto the socket before it is used.
    ///
    /// `secure` runs immediately after `bind`, before the socket is made
    /// non-blocking or given `SO_PASSCRED` and before the caller can poll it.
    /// That is as early as it can run: `bind` is what publishes a pathname
    /// socket, and a datagram socket has no `listen` to hold it back.
    ///
    /// # Why the descriptor is installed by path
    ///
    /// It addresses the socket by path rather than by descriptor because **the
    /// fd form does not work on a socket**. `peios::file::fd_set_sd` is the
    /// path syscall with the target as `dirfd`, an empty path and
    /// `AT_EMPTY_PATH`, and the kernel answers `EOPNOTSUPP` for a socket fd.
    /// Tried, and it took PID 1 into recovery.
    ///
    /// So a window does exist, between `bind` and this call, in which the
    /// socket carries what it inherited. It is safe here for the same reason
    /// it is on the control socket: the parent directory grants services
    /// traverse and nothing else, and carries no inheritable ACE, so what the
    /// socket inherits is narrower than what is installed. The failure
    /// direction is refusal, not exposure.
    pub fn bind_secured(
        path: impl AsRef<Path>,
        secure: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<Self, NotifySocketBindError> {
        let path = path.as_ref().to_path_buf();
        unlink_stale_path(&path)?;
        let socket = UnixDatagram::bind(&path).map_err(|source| NotifySocketBindError::Bind {
            path: path.clone(),
            source,
        })?;
        secure(&path).map_err(NotifySocketBindError::Secure)?;
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
