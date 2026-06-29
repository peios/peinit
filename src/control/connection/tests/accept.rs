use std::collections::VecDeque;
use std::io;

use crate::control::connection::{
    ControlAcceptedConnection, ControlConnectionAcceptError, ControlConnectionAcceptTurn,
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable,
    ControlConnectionTableError, ControlListener, accept_control_connection,
};
use crate::control::socket::{
    ControlSocketAcceptError, ControlSocketRead, ControlSocketReadError, ControlSocketWrite,
    ControlSocketWriteError,
};
use crate::control::system::{ControlPeer, SystemAccessCheckError};
use crate::security::TokenSummary;

#[test]
fn accept_turn_reports_would_block_without_mutating_table() {
    let mut listener = FakeControlListener::accepts([FakeAccept::WouldBlock]);
    let mut table = ControlConnectionTable::new(2);

    let turn = accept_control_connection(&mut listener, &mut table).expect("accept");

    assert_eq!(turn, ControlConnectionAcceptTurn::WouldBlock);
    assert_eq!(listener.accept_calls, 1);
    assert!(table.is_empty());
}

#[test]
fn accept_turn_inserts_authenticated_connection_record() {
    let mut listener = FakeControlListener::accepts([FakeAccept::Connection(
        FakeAcceptedConnection::new(42, "admin"),
    )]);
    let mut table = ControlConnectionTable::new(2);

    let turn = accept_control_connection(&mut listener, &mut table).expect("accept");

    assert_eq!(
        turn,
        ControlConnectionAcceptTurn::Accepted {
            fd: 42,
            active_connections: 1,
        },
    );
    let record = table.get(42).expect("accepted record");
    assert_eq!(record.io().fd, 42);
    assert_eq!(record.peer().token_fd(), 1042);
    assert_eq!(record.peer().summary.identity, "admin");
    assert_eq!(record.state().pending_write_bytes(), 0);
}

#[test]
fn accept_turn_rejects_connection_when_peer_lookup_fails() {
    let peer_error = SystemAccessCheckError::Boundary("open peer failed".to_string());
    let mut listener = FakeControlListener::accepts([FakeAccept::Connection(
        FakeAcceptedConnection::peer_error(52, peer_error.clone()),
    )]);
    let mut table = ControlConnectionTable::new(2);

    let turn = accept_control_connection(&mut listener, &mut table).expect("accept");

    assert_eq!(
        turn,
        ControlConnectionAcceptTurn::PeerRejected {
            fd: 52,
            error: peer_error,
        },
    );
    assert!(table.is_empty());
}

#[test]
fn accept_turn_rejects_at_socket_when_connection_table_is_full() {
    let mut listener = FakeControlListener::accepts([FakeAccept::Connection(
        FakeAcceptedConnection::new(31, "second"),
    )]);
    let mut table = ControlConnectionTable::new(1);
    table
        .admit(30, accepted_record(30, "first"))
        .expect("seed connection");

    let turn = accept_control_connection(&mut listener, &mut table).expect("accept");

    assert_eq!(
        turn,
        ControlConnectionAcceptTurn::RejectedAtSocket {
            fd: 31,
            active_connections: 1,
            max_connections: 1,
        },
    );
    assert_eq!(table.len(), 1);
    assert!(table.get(31).is_none());
}

#[test]
fn accept_turn_reports_duplicate_connection_fd_as_table_error() {
    let mut listener = FakeControlListener::accepts([FakeAccept::Connection(
        FakeAcceptedConnection::new(44, "duplicate"),
    )]);
    let mut table = ControlConnectionTable::new(2);
    table
        .admit(44, accepted_record(44, "first"))
        .expect("seed connection");

    let err = accept_control_connection(&mut listener, &mut table).expect_err("duplicate fd");

    assert!(matches!(
        err,
        ControlConnectionAcceptError::Table(ControlConnectionTableError::AlreadyTracked { fd: 44 }),
    ));
    assert_eq!(table.len(), 1);
}

#[test]
fn accept_turn_wraps_listener_socket_error() {
    let mut listener =
        FakeControlListener::accepts([FakeAccept::SocketError(io::ErrorKind::ConnectionAborted)]);
    let mut table: ControlConnectionTable<ControlConnectionRecord<FakeAcceptedConnection>> =
        ControlConnectionTable::new(2);

    let err = accept_control_connection(&mut listener, &mut table).expect_err("socket error");

    assert!(matches!(err, ControlConnectionAcceptError::Socket(_)));
    assert!(table.is_empty());
}

#[derive(Debug)]
enum FakeAccept {
    Connection(FakeAcceptedConnection),
    WouldBlock,
    SocketError(io::ErrorKind),
}

#[derive(Debug, Default)]
struct FakeControlListener {
    accepts: VecDeque<FakeAccept>,
    accept_calls: usize,
}

impl FakeControlListener {
    fn accepts(accepts: impl IntoIterator<Item = FakeAccept>) -> Self {
        Self {
            accepts: accepts.into_iter().collect(),
            ..Self::default()
        }
    }
}

impl ControlListener for FakeControlListener {
    type Connection = FakeAcceptedConnection;

    fn accept_control(&mut self) -> Result<Option<Self::Connection>, ControlSocketAcceptError> {
        self.accept_calls += 1;
        match self.accepts.pop_front().unwrap_or(FakeAccept::WouldBlock) {
            FakeAccept::Connection(connection) => Ok(Some(connection)),
            FakeAccept::WouldBlock => Ok(None),
            FakeAccept::SocketError(kind) => {
                Err(ControlSocketAcceptError::Accept(io::Error::from(kind)))
            }
        }
    }
}

#[derive(Debug)]
struct FakeAcceptedConnection {
    fd: i32,
    peer_identity: Option<String>,
    peer_error: Option<SystemAccessCheckError>,
}

impl FakeAcceptedConnection {
    fn new(fd: i32, peer_identity: impl Into<String>) -> Self {
        Self {
            fd,
            peer_identity: Some(peer_identity.into()),
            peer_error: None,
        }
    }

    fn peer_error(fd: i32, error: SystemAccessCheckError) -> Self {
        Self {
            fd,
            peer_identity: None,
            peer_error: Some(error),
        }
    }
}

impl ControlConnectionIo for FakeAcceptedConnection {
    fn read_control(
        &mut self,
        _max_bytes: usize,
    ) -> Result<ControlSocketRead, ControlSocketReadError> {
        Ok(ControlSocketRead::Eof)
    }

    fn write_control(
        &mut self,
        _bytes: &[u8],
    ) -> Result<ControlSocketWrite, ControlSocketWriteError> {
        Ok(ControlSocketWrite::Complete)
    }
}

impl ControlAcceptedConnection for FakeAcceptedConnection {
    fn control_fd(&self) -> i32 {
        self.fd
    }

    fn control_peer(&self) -> Result<ControlPeer, SystemAccessCheckError> {
        if let Some(error) = &self.peer_error {
            return Err(error.clone());
        }
        Ok(control_peer_for(
            self.fd,
            self.peer_identity
                .as_deref()
                .expect("fake connection has peer identity"),
        ))
    }
}

fn accepted_record(
    fd: i32,
    peer_identity: impl Into<String>,
) -> ControlConnectionRecord<FakeAcceptedConnection> {
    let peer_identity = peer_identity.into();
    ControlConnectionRecord::new(
        FakeAcceptedConnection::new(fd, peer_identity.clone()),
        control_peer_for(fd, &peer_identity),
    )
}

fn control_peer_for(fd: i32, identity: &str) -> ControlPeer {
    ControlPeer::borrowed_token_fd(1000 + fd, TokenSummary::requested_identity(identity))
}
