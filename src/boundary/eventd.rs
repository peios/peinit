use std::os::unix::net::UnixDatagram;

use crate::logging::{ServiceLogRecord, encode_eventd_log_records_into};

use super::BoundaryError;

/// What became of one datagram.
///
/// The distinction is the whole point of §12.1's lossy design: log ingestion
/// must not exert backpressure on senders, so eventd's `SO_RCVBUF` filling is
/// a *drop*, not a transport failure. Treating it as an error made peinit
/// clear its socket path and push the batch back into the pre-eventd buffer,
/// then re-enable forwarding at the end of the turn and replay — so the system
/// oscillated between forwarding and buffering under exactly the load the
/// design exists to absorb, and re-sent records eventd may already have held
/// (PEI-357).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventdSendOutcome {
    Sent,
    /// The kernel discarded it because the receiver's buffer is full. Expected
    /// under load, and the connection is still good.
    Dropped,
}

pub trait EventdLogSink {
    fn send_eventd_log_records(
        &mut self,
        socket_path: &str,
        records: &[ServiceLogRecord],
    ) -> Result<EventdSendOutcome, BoundaryError>;
}

#[derive(Debug)]
struct ConnectedEventdLogSocket {
    path: Box<str>,
    socket: UnixDatagram,
}

/// A persistent, non-blocking eventd sender.
///
/// The socket and encoding allocation are both retained across datagrams. A
/// failed send invalidates the connection because eventd may have restarted
/// and rebound the same filesystem path.
#[derive(Debug, Default)]
pub struct LinuxEventdLogSink {
    connected: Option<ConnectedEventdLogSocket>,
    payload: Vec<u8>,
}

impl LinuxEventdLogSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn disconnect(&mut self) {
        self.connected = None;
    }

    fn connect(&mut self, socket_path: &str) -> Result<(), BoundaryError> {
        if self
            .connected
            .as_ref()
            .is_some_and(|connected| connected.path.as_ref() == socket_path)
        {
            return Ok(());
        }

        let socket = UnixDatagram::unbound().map_err(|error| {
            BoundaryError::EventdLog(format!("create datagram socket: {error}"))
        })?;
        socket
            .set_nonblocking(true)
            .map_err(|error| BoundaryError::EventdLog(format!("set nonblocking: {error}")))?;
        socket.connect(socket_path).map_err(|error| {
            BoundaryError::EventdLog(format!("connect to {socket_path}: {error}"))
        })?;
        self.connected = Some(ConnectedEventdLogSocket {
            path: socket_path.into(),
            socket,
        });
        Ok(())
    }
}

impl EventdLogSink for LinuxEventdLogSink {
    fn send_eventd_log_records(
        &mut self,
        socket_path: &str,
        records: &[ServiceLogRecord],
    ) -> Result<EventdSendOutcome, BoundaryError> {
        if records.is_empty() {
            return Ok(EventdSendOutcome::Sent);
        }
        self.connect(socket_path)?;
        encode_eventd_log_records_into(&mut self.payload, records);

        let sent = match self
            .connected
            .as_ref()
            .expect("eventd socket connected above")
            .socket
            .send(&self.payload)
        {
            Ok(sent) => sent,
            // The receive buffer is full, or the socket would block. The
            // datagram is gone and that is the designed outcome; the
            // connection is untouched, because nothing about it is wrong.
            Err(error) if is_receiver_full(&error) => return Ok(EventdSendOutcome::Dropped),
            Err(error) => {
                self.connected = None;
                return Err(BoundaryError::EventdLog(format!(
                    "send to {socket_path}: {error}"
                )));
            }
        };
        if sent == self.payload.len() {
            Ok(EventdSendOutcome::Sent)
        } else {
            self.connected = None;
            Err(BoundaryError::EventdLog(format!(
                "partial datagram send to {socket_path}: {sent}/{} bytes",
                self.payload.len()
            )))
        }
    }
}

/// A full receiver, as opposed to a broken one.
///
/// `EAGAIN` on a non-blocking socket and `ENOBUFS` both mean the datagram did
/// not fit. Everything else — the path gone, the peer refusing — is a
/// transport failure and does invalidate the connection, because eventd may
/// have restarted and rebound the same filesystem path.
///
/// `EWOULDBLOCK` is not listed separately: on Linux it is the same value as
/// `EAGAIN`, so naming both is an unreachable arm rather than extra coverage.
fn is_receiver_full(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EAGAIN) | Some(libc::ENOBUFS)
    )
}

pub fn send_eventd_log_record(
    socket_path: &str,
    record: &ServiceLogRecord,
) -> Result<EventdSendOutcome, BoundaryError> {
    LinuxEventdLogSink::new().send_eventd_log_records(socket_path, std::slice::from_ref(record))
}

#[cfg(test)]
mod tests;
