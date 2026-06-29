use crate::execution::notify::{AuthenticatedNotifySender, NotifyApplyError};
use crate::notify::{NotifyDatagram, NotifyParseError, NotifySocket, NotifySocketReadError};
use crate::shutdown::ShutdownError;
use crate::supervisor::SupervisorNotifyDispatch;

pub trait RuntimeNotifySource {
    fn read_notify_datagram(&mut self) -> Result<Option<NotifyDatagram>, NotifySocketReadError>;
}

impl RuntimeNotifySource for NotifySocket {
    fn read_notify_datagram(&mut self) -> Result<Option<NotifyDatagram>, NotifySocketReadError> {
        match self.receive() {
            Ok(datagram) => Ok(Some(datagram)),
            Err(NotifySocketReadError::WouldBlock) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeNotifyRead {
    Datagram(RuntimeNotifyDatagram),
    WouldBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeNotifyDatagram {
    pub sender_pid: u32,
    pub payload: Vec<u8>,
    pub fd_count: usize,
}

impl RuntimeNotifyDatagram {
    pub fn from_datagram(datagram: &NotifyDatagram) -> Self {
        Self {
            sender_pid: datagram.credentials.pid,
            payload: datagram.payload.clone(),
            fd_count: datagram.fds.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeNotifySupervisorTurn {
    Applied(Box<SupervisorNotifyDispatch>),
    Rejected(RuntimeNotifyRejection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeNotifyRejection {
    Parse {
        error: NotifyParseError,
        attribution: Option<AuthenticatedNotifySender>,
    },
    Apply {
        error: NotifyApplyError,
        attribution: Option<AuthenticatedNotifySender>,
    },
    Shutdown(ShutdownError),
}
