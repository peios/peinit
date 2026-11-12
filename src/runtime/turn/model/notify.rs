use crate::execution::notify::{AuthenticatedNotifySender, NotifyApplyError};
use crate::notify::{NotifyDatagram, NotifyParseError, NotifySocket, NotifySocketReadError};
use crate::shutdown::ShutdownError;
use crate::supervisor::{SupervisorNotifyDispatch, SupervisorSubmittedNotifyDispatch};

pub trait RuntimeNotifySource {
    fn read_notify_datagram(&mut self) -> Result<Option<NotifyDatagram>, NotifySocketReadError>;

    /// Datagrams read before their sender's setup status could be (PEI-1688).
    fn held_notify_datagrams(&mut self) -> &mut RuntimeHeldNotifyDatagrams;
}

/// The notification socket, with the datagrams it is holding for a sender
/// whose job does not carry its PID yet.
#[derive(Debug)]
pub struct RuntimeNotifySocket {
    socket: NotifySocket,
    held: RuntimeHeldNotifyDatagrams,
}

impl RuntimeNotifySocket {
    pub fn new(socket: NotifySocket) -> Self {
        Self {
            socket,
            held: RuntimeHeldNotifyDatagrams::default(),
        }
    }

    pub fn path(&self) -> &std::path::Path {
        self.socket.path()
    }
}

impl RuntimeNotifySource for RuntimeNotifySocket {
    fn read_notify_datagram(&mut self) -> Result<Option<NotifyDatagram>, NotifySocketReadError> {
        match self.socket.receive() {
            Ok(datagram) => Ok(Some(datagram)),
            Err(NotifySocketReadError::WouldBlock) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn held_notify_datagrams(&mut self) -> &mut RuntimeHeldNotifyDatagrams {
        &mut self.held
    }
}

/// Datagrams from a launched process whose setup status could not be read
/// when they arrived, in arrival order.
///
/// A job carries its PID only once its setup status is processed, so a
/// datagram from such a process cannot be authenticated yet; dropping it is
/// unrecoverable, because a `READY=1` is sent once (PEI-1688). The runtime
/// reads the setup status first, and that is enough whenever the status is
/// readable — it always is once the process has exec'd. This is the fallback
/// for when it is not, so it is bounded per sender rather than sized for use.
#[derive(Debug, Default)]
pub struct RuntimeHeldNotifyDatagrams {
    datagrams: Vec<NotifyDatagram>,
}

impl RuntimeHeldNotifyDatagrams {
    /// The most datagrams held for one sender; more are rejected.
    pub const MAX_PER_SENDER: usize = 16;

    /// Hold `datagram`, or hand it back if its sender already has
    /// [`Self::MAX_PER_SENDER`] held.
    pub fn hold(&mut self, datagram: NotifyDatagram) -> Result<(), NotifyDatagram> {
        let sender_pid = datagram.credentials.pid;
        let held = self
            .datagrams
            .iter()
            .filter(|held| held.credentials.pid == sender_pid)
            .count();
        if held >= Self::MAX_PER_SENDER {
            return Err(datagram);
        }
        self.datagrams.push(datagram);
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.datagrams.is_empty()
    }

    /// Take, in arrival order, every held datagram whose sender `still_held`
    /// no longer keeps.
    pub fn release(&mut self, mut still_held: impl FnMut(u32) -> bool) -> Vec<NotifyDatagram> {
        let (kept, released) = std::mem::take(&mut self.datagrams)
            .into_iter()
            .partition(|datagram| still_held(datagram.credentials.pid));
        self.datagrams = kept;
        released
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
    AppliedToJob(Box<SupervisorSubmittedNotifyDispatch>),
    Rejected(RuntimeNotifyRejection),
    /// The sender's setup status could not be read yet, so its job does not
    /// carry its PID; the datagram is held, and applied once the setup is
    /// processed (PEI-1688).
    HeldUntilSetup {
        sender_pid: u32,
    },
    /// Applying the datagram raised an internal error, contained to the
    /// sender's service (PEI-1125).
    InternalError(Box<crate::supervisor::SupervisorInternalErrorDispatch>),
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
    /// The kernel truncated the datagram, so no field from it was applied.
    Truncated {
        payload: bool,
        control: bool,
    },
    /// The sender's setup status still could not be read, and it already has
    /// as many datagrams held as are allowed.
    SetupPending,
}
