use std::collections::VecDeque;
use std::io;

use crate::control::connection::{ControlAcceptedConnection, ControlConnectionIo, ControlListener};
use crate::control::socket::{
    ControlSocketAcceptError, ControlSocketRead, ControlSocketReadError, ControlSocketWrite,
    ControlSocketWriteError,
};
use crate::control::system::{ControlPeer, SystemAccessCheckError};
use crate::security::TokenSummary;

#[derive(Debug, Default)]
pub(crate) struct FakeControlListener {
    accepts: VecDeque<FakeAcceptedConnection>,
}

impl FakeControlListener {
    pub(crate) fn accepts(accepts: impl IntoIterator<Item = FakeAcceptedConnection>) -> Self {
        Self {
            accepts: accepts.into_iter().collect(),
        }
    }
}

impl ControlListener for FakeControlListener {
    type Connection = FakeAcceptedConnection;

    fn accept_control(&mut self) -> Result<Option<Self::Connection>, ControlSocketAcceptError> {
        Ok(self.accepts.pop_front())
    }
}

#[derive(Debug)]
pub(crate) struct FakeAcceptedConnection {
    fd: i32,
    peer_identity: String,
    reads: VecDeque<ControlSocketRead>,
    write_results: VecDeque<ControlSocketWrite>,
    writes: Vec<Vec<u8>>,
}

impl FakeAcceptedConnection {
    pub(crate) fn new(fd: i32, peer_identity: impl Into<String>) -> Self {
        Self::with_io(fd, peer_identity, [], [])
    }

    pub(crate) fn with_io(
        fd: i32,
        peer_identity: impl Into<String>,
        reads: impl IntoIterator<Item = ControlSocketRead>,
        writes: impl IntoIterator<Item = ControlSocketWrite>,
    ) -> Self {
        Self {
            fd,
            peer_identity: peer_identity.into(),
            reads: reads.into_iter().collect(),
            write_results: writes.into_iter().collect(),
            writes: Vec::new(),
        }
    }
}

impl ControlConnectionIo for FakeAcceptedConnection {
    fn read_control(
        &mut self,
        _max_bytes: usize,
    ) -> Result<ControlSocketRead, ControlSocketReadError> {
        Ok(self
            .reads
            .pop_front()
            .unwrap_or(ControlSocketRead::WouldBlock))
    }

    fn write_control(
        &mut self,
        bytes: &[u8],
    ) -> Result<ControlSocketWrite, ControlSocketWriteError> {
        self.writes.push(bytes.to_vec());
        self.write_results.pop_front().ok_or_else(|| {
            ControlSocketWriteError::Write(io::Error::new(
                io::ErrorKind::WouldBlock,
                "scripted write exhausted",
            ))
        })
    }
}

impl ControlAcceptedConnection for FakeAcceptedConnection {
    fn control_fd(&self) -> i32 {
        self.fd
    }

    fn control_peer(&self) -> Result<ControlPeer, SystemAccessCheckError> {
        Ok(control_peer(&self.peer_identity))
    }
}

pub(crate) fn control_peer(identity: &str) -> ControlPeer {
    ControlPeer::borrowed_token_fd(44, TokenSummary::requested_identity(identity))
}
