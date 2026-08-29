use std::os::fd::{AsRawFd, OwnedFd};

use super::model::{JobsSocketRead, JobsSocketReadError, JobsSocketWrite, JobsSocketWriteError};

/// One accepted jobs connection.
#[derive(Debug)]
pub struct LinuxJobsConnection {
    fd: OwnedFd,
}

impl LinuxJobsConnection {
    pub(super) fn from_owned(fd: OwnedFd) -> Self {
        Self { fd }
    }

    pub fn as_raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    /// The submitter's identity and the kernel's handle on its process, both
    /// captured once at accept (PSPU §7.5).
    #[cfg(feature = "peios-boundary")]
    pub fn peer(
        &self,
    ) -> Result<crate::jobs::connection::JobsPeer, crate::control::system::SystemAccessCheckError>
    {
        use std::os::fd::AsFd;

        use crate::control::system::{
            SystemAccessCheckError, peios_control_peer_from_connected_socket,
        };

        let control = peios_control_peer_from_connected_socket(self.fd.as_fd())?;
        let pidfd = peios::socket::peer_pidfd(self.fd.as_fd())
            .map_err(|error| SystemAccessCheckError::Boundary(format!("peer pidfd: {error}")))?;
        Ok(crate::jobs::connection::JobsPeer { control, pidfd })
    }

    /// Receive one record with room for one token and `max_descriptors`
    /// descriptors. Everything the kernel delivered is owned by the result.
    #[cfg(feature = "peios-boundary")]
    pub fn receive(
        &self,
        max_bytes: usize,
        max_descriptors: usize,
    ) -> Result<JobsSocketRead, JobsSocketReadError> {
        use std::os::fd::AsFd;

        let mut buffer = vec![0_u8; max_bytes];
        let received = match peios::socket::recv_message(
            self.fd.as_fd(),
            &mut buffer,
            max_descriptors,
            libc::MSG_DONTWAIT,
        ) {
            Ok(received) => received,
            Err(error) if error.raw_os_error() == Some(libc::EAGAIN) => {
                return Ok(JobsSocketRead::WouldBlock);
            }
            Err(error) => return Err(JobsSocketReadError::Recv(error.into())),
        };
        if received.len == 0 && received.token.is_none() && received.fds.is_empty() {
            return Ok(JobsSocketRead::Eof);
        }
        buffer.truncate(received.len);
        Ok(JobsSocketRead::Message(super::model::JobsMessage {
            payload: buffer,
            token: received.token.map(OwnedFd::from),
            descriptors: received.fds,
            truncated: received.truncated,
            control_truncated: received.control_truncated,
        }))
    }

    #[cfg(not(feature = "peios-boundary"))]
    pub fn receive(
        &self,
        _max_bytes: usize,
        _max_descriptors: usize,
    ) -> Result<JobsSocketRead, JobsSocketReadError> {
        Err(JobsSocketReadError::Recv(std::io::Error::other(
            "jobs socket receive requires the peios boundary",
        )))
    }

    /// Send one record, attaching `fd` with `SCM_RIGHTS` when given. A
    /// sequenced-packet record goes whole or not at all.
    pub fn send(
        &self,
        bytes: &[u8],
        fd: Option<i32>,
    ) -> Result<JobsSocketWrite, JobsSocketWriteError> {
        #[cfg(feature = "peios-boundary")]
        {
            use std::os::fd::{AsFd, BorrowedFd};

            let attached = fd.map(|fd| unsafe { BorrowedFd::borrow_raw(fd) });
            let fds: Vec<BorrowedFd<'_>> = attached.into_iter().collect();
            match peios::socket::send_message(
                self.fd.as_fd(),
                bytes,
                None,
                &fds,
                libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
            ) {
                Ok(_) => Ok(JobsSocketWrite::Complete),
                Err(error) if error.raw_os_error() == Some(libc::EAGAIN) => {
                    Ok(JobsSocketWrite::WouldBlock)
                }
                Err(error) => Err(JobsSocketWriteError::Send(error.into())),
            }
        }
        #[cfg(not(feature = "peios-boundary"))]
        {
            let _ = (bytes, fd);
            Err(JobsSocketWriteError::Send(std::io::Error::other(
                "jobs socket send requires the peios boundary",
            )))
        }
    }
}
