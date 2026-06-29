use std::io;
use std::os::fd::{AsRawFd, OwnedFd};

use super::{
    ControlSocketRead, ControlSocketReadError, ControlSocketWrite, ControlSocketWriteError,
};

#[derive(Debug)]
pub struct LinuxControlConnection {
    pub(super) fd: OwnedFd,
}

impl LinuxControlConnection {
    pub fn as_raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    #[cfg(feature = "peios-boundary")]
    pub fn peer(
        &self,
    ) -> Result<crate::control::system::ControlPeer, crate::control::system::SystemAccessCheckError>
    {
        use std::os::fd::AsFd;

        crate::control::system::peios_control_peer_from_connected_socket(self.fd.as_fd())
    }

    pub fn read(&self, max_bytes: usize) -> Result<ControlSocketRead, ControlSocketReadError> {
        let mut buffer = vec![0_u8; max_bytes];
        let rc = unsafe {
            libc::read(
                self.fd.as_raw_fd(),
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
            )
        };
        if rc == 0 {
            return Ok(ControlSocketRead::Eof);
        }
        if rc < 0 {
            let error = io::Error::last_os_error();
            if is_would_block(&error) {
                return Ok(ControlSocketRead::WouldBlock);
            }
            return Err(ControlSocketReadError::Read(error));
        }
        buffer.truncate(rc as usize);
        Ok(ControlSocketRead::Bytes(buffer))
    }

    pub fn write(&self, bytes: &[u8]) -> Result<ControlSocketWrite, ControlSocketWriteError> {
        let rc = unsafe {
            libc::write(
                self.fd.as_raw_fd(),
                bytes.as_ptr().cast::<libc::c_void>(),
                bytes.len(),
            )
        };
        if rc < 0 {
            let error = io::Error::last_os_error();
            if is_would_block(&error) {
                return Ok(ControlSocketWrite::WouldBlock);
            }
            return Err(ControlSocketWriteError::Write(error));
        }
        let written = rc as usize;
        if written == bytes.len() {
            Ok(ControlSocketWrite::Complete)
        } else if written == 0 {
            Ok(ControlSocketWrite::WouldBlock)
        } else {
            Ok(ControlSocketWrite::Partial { written })
        }
    }
}

pub(super) fn is_would_block(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK
    )
}
