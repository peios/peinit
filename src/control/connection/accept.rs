use crate::control::socket::ControlSocketAcceptError;
#[cfg(feature = "peios-boundary")]
use crate::control::socket::{LinuxControlConnection, LinuxControlSocket};
use crate::control::system::{ControlPeer, SystemAccessCheckError};

use super::{
    ControlConnectionAdmission, ControlConnectionIo, ControlConnectionRecord,
    ControlConnectionTable, ControlConnectionTableError,
};

pub trait ControlAcceptedConnection: ControlConnectionIo {
    fn control_fd(&self) -> i32;

    fn control_peer(&self) -> Result<ControlPeer, SystemAccessCheckError>;
}

pub trait ControlListener {
    type Connection: ControlAcceptedConnection;

    fn accept_control(&mut self) -> Result<Option<Self::Connection>, ControlSocketAcceptError>;
}

#[cfg(feature = "peios-boundary")]
impl ControlAcceptedConnection for LinuxControlConnection {
    fn control_fd(&self) -> i32 {
        self.as_raw_fd()
    }

    fn control_peer(&self) -> Result<ControlPeer, SystemAccessCheckError> {
        self.peer()
    }
}

#[cfg(feature = "peios-boundary")]
impl ControlListener for LinuxControlSocket {
    type Connection = LinuxControlConnection;

    fn accept_control(&mut self) -> Result<Option<Self::Connection>, ControlSocketAcceptError> {
        self.accept()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlConnectionAcceptTurn {
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
pub enum ControlConnectionAcceptError {
    Socket(ControlSocketAcceptError),
    Table(ControlConnectionTableError),
}

pub fn accept_control_connection<L>(
    listener: &mut L,
    table: &mut ControlConnectionTable<ControlConnectionRecord<L::Connection>>,
) -> Result<ControlConnectionAcceptTurn, ControlConnectionAcceptError>
where
    L: ControlListener + ?Sized,
{
    accept_control_connection_at(listener, table, None)
}

pub fn accept_control_connection_at<L>(
    listener: &mut L,
    table: &mut ControlConnectionTable<ControlConnectionRecord<L::Connection>>,
    observed_at_ns: Option<u64>,
) -> Result<ControlConnectionAcceptTurn, ControlConnectionAcceptError>
where
    L: ControlListener + ?Sized,
{
    let Some(connection) = listener
        .accept_control()
        .map_err(ControlConnectionAcceptError::Socket)?
    else {
        return Ok(ControlConnectionAcceptTurn::WouldBlock);
    };
    let fd = connection.control_fd();
    let peer = match connection.control_peer() {
        Ok(peer) => peer,
        Err(error) => return Ok(ControlConnectionAcceptTurn::PeerRejected { fd, error }),
    };

    let record = ControlConnectionRecord::new_with_activity(connection, peer, observed_at_ns);
    match table
        .admit(fd, record)
        .map_err(ControlConnectionAcceptError::Table)?
    {
        ControlConnectionAdmission::Accepted {
            active_connections, ..
        } => Ok(ControlConnectionAcceptTurn::Accepted {
            fd,
            active_connections,
        }),
        ControlConnectionAdmission::RejectedAtSocket {
            active_connections,
            max_connections,
            ..
        } => Ok(ControlConnectionAcceptTurn::RejectedAtSocket {
            fd,
            active_connections,
            max_connections,
        }),
    }
}
