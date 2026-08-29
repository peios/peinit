use std::os::fd::OwnedFd;

use crate::control::connection::{ControlConnectionAdmission, ControlConnectionTableError};
use crate::control::system::{ControlPeer, SystemAccessCheckError};
use crate::jobs::socket::{JobsSocketAcceptError, LinuxJobsConnection, LinuxJobsSocket};

use super::record::{JobsConnectionIo, JobsConnectionRecord};
use super::table::JobsConnectionTable;

/// What accept captured about the submitter: its token, and the kernel's
/// handle on its process for the peer-primary identity path.
#[derive(Debug)]
pub struct JobsPeer {
    pub control: ControlPeer,
    pub pidfd: OwnedFd,
}

pub trait JobsAcceptedConnection: JobsConnectionIo {
    fn jobs_fd(&self) -> i32;

    fn jobs_peer(&self) -> Result<JobsPeer, SystemAccessCheckError>;
}

pub trait JobsListener {
    type Connection: JobsAcceptedConnection;

    fn accept_jobs(&mut self) -> Result<Option<Self::Connection>, JobsSocketAcceptError>;
}

impl JobsAcceptedConnection for LinuxJobsConnection {
    fn jobs_fd(&self) -> i32 {
        self.as_raw_fd()
    }

    #[cfg(feature = "peios-boundary")]
    fn jobs_peer(&self) -> Result<JobsPeer, SystemAccessCheckError> {
        self.peer()
    }

    #[cfg(not(feature = "peios-boundary"))]
    fn jobs_peer(&self) -> Result<JobsPeer, SystemAccessCheckError> {
        Err(SystemAccessCheckError::Boundary(
            "jobs peer identity requires the peios boundary".to_string(),
        ))
    }
}

impl JobsListener for LinuxJobsSocket {
    type Connection = LinuxJobsConnection;

    fn accept_jobs(&mut self) -> Result<Option<Self::Connection>, JobsSocketAcceptError> {
        self.accept()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobsConnectionAcceptTurn {
    Accepted {
        fd: i32,
        active_connections: usize,
    },
    RejectedAtSocket {
        fd: i32,
        active_connections: usize,
        max_connections: usize,
    },
    PeerRejected {
        fd: i32,
        error: SystemAccessCheckError,
    },
    WouldBlock,
}

#[derive(Debug)]
pub enum JobsConnectionAcceptError {
    Socket(JobsSocketAcceptError),
    Table(ControlConnectionTableError),
}

/// Accept one connection, capture the peer, admit against the limit. A peer
/// that cannot be identified is closed without a response (PSPU §4.6).
pub fn accept_jobs_connection_at<L>(
    listener: &mut L,
    table: &mut JobsConnectionTable<L::Connection>,
    observed_at_ns: Option<u64>,
) -> Result<JobsConnectionAcceptTurn, JobsConnectionAcceptError>
where
    L: JobsListener + ?Sized,
{
    let Some(connection) = listener
        .accept_jobs()
        .map_err(JobsConnectionAcceptError::Socket)?
    else {
        return Ok(JobsConnectionAcceptTurn::WouldBlock);
    };
    let fd = connection.jobs_fd();
    let peer = match connection.jobs_peer() {
        Ok(peer) => peer,
        Err(error) => return Ok(JobsConnectionAcceptTurn::PeerRejected { fd, error }),
    };
    let record = JobsConnectionRecord::new_with_activity(connection, peer, observed_at_ns);
    match table
        .admit(fd, record)
        .map_err(JobsConnectionAcceptError::Table)?
    {
        ControlConnectionAdmission::Accepted {
            active_connections, ..
        } => Ok(JobsConnectionAcceptTurn::Accepted {
            fd,
            active_connections,
        }),
        ControlConnectionAdmission::RejectedAtSocket {
            active_connections,
            max_connections,
            ..
        } => Ok(JobsConnectionAcceptTurn::RejectedAtSocket {
            fd,
            active_connections,
            max_connections,
        }),
    }
}
