use std::os::fd::AsRawFd;
use std::os::unix::net::UnixDatagram;

use crate::logging::{
    DEFAULT_EVENTD_LOG_DATAGRAM_BYTES, ServiceLogRecord, encode_eventd_log_records_into,
};

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
    /// The datagram is larger than the socket can carry (`EMSGSIZE`). The
    /// connection is still good, but retrying the same batch can never
    /// succeed: it is the batch that has to go, not the socket (PEI-807).
    Oversized,
}

pub trait EventdLogSink {
    fn send_eventd_log_records(
        &mut self,
        socket_path: &str,
        records: &[ServiceLogRecord],
    ) -> Result<EventdSendOutcome, BoundaryError>;

    /// The largest datagram the socket to `socket_path` can carry, if the
    /// sink knows it. A batch built to the portable ceiling alone was larger
    /// than a default `SO_SNDBUF`, so the caller bounds each batch by this as
    /// well (PEI-807). `None` means "no tighter bound than the caller's own".
    fn eventd_datagram_ceiling(
        &mut self,
        socket_path: &str,
    ) -> Result<Option<usize>, BoundaryError> {
        let _ = socket_path;
        Ok(None)
    }
}

#[derive(Debug)]
struct ConnectedEventdLogSocket {
    path: Box<str>,
    socket: UnixDatagram,
    /// The largest datagram this socket's send buffer admits, read back from
    /// the kernel after asking for the portable ceiling.
    max_datagram_bytes: usize,
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

/// What `unix_dgram_sendmsg` keeps back from `SO_SNDBUF`: a datagram longer
/// than `sk_sndbuf - 32` is refused with `EMSGSIZE` before anything is
/// queued. The 32 is the kernel's own constant, not a tunable.
const UNIX_DATAGRAM_SNDBUF_RESERVE: usize = 32;

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
        // A Unix datagram socket's default send buffer (212992 bytes on a
        // stock kernel) is smaller than the 262144-byte portable ceiling the
        // batches are built to, so a full batch failed with EMSGSIZE. Ask for
        // the ceiling; the kernel grants what `wmem_max` allows, and the
        // read-back below is the truth either way (PEI-807).
        let max_datagram_bytes = raise_send_buffer(&socket, DEFAULT_EVENTD_LOG_DATAGRAM_BYTES)
            .map_err(|error| BoundaryError::EventdLog(format!("size send buffer: {error}")))?;
        socket.connect(socket_path).map_err(|error| {
            BoundaryError::EventdLog(format!("connect to {socket_path}: {error}"))
        })?;
        self.connected = Some(ConnectedEventdLogSocket {
            path: socket_path.into(),
            socket,
            max_datagram_bytes,
        });
        Ok(())
    }
}

/// Ask for `requested` bytes of send buffer and return the largest datagram
/// the buffer actually granted will carry. The request is best effort -- the
/// kernel silently caps it at `wmem_max` -- which is why the answer comes
/// from reading the option back rather than from the request.
fn raise_send_buffer(socket: &UnixDatagram, requested: usize) -> std::io::Result<usize> {
    let fd = socket.as_raw_fd();
    let requested = libc::c_int::try_from(requested).unwrap_or(libc::c_int::MAX);
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&requested as *const libc::c_int).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut granted: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&mut granted as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(usize::try_from(granted)
        .unwrap_or(0)
        .saturating_sub(UNIX_DATAGRAM_SNDBUF_RESERVE))
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
            // Too big for the socket. Also not a fault of the connection --
            // and, unlike every other failure, one that replaying the same
            // batch reproduces exactly. Classed as a transport failure it
            // cleared the socket path, kept the records, and replayed the
            // identical batch every turn for the rest of the boot (PEI-807).
            Err(error) if is_oversized(&error) => return Ok(EventdSendOutcome::Oversized),
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

    fn eventd_datagram_ceiling(
        &mut self,
        socket_path: &str,
    ) -> Result<Option<usize>, BoundaryError> {
        self.connect(socket_path)?;
        Ok(self
            .connected
            .as_ref()
            .map(|connected| connected.max_datagram_bytes))
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

/// A datagram the socket cannot carry at any time, as opposed to one it
/// cannot carry right now.
fn is_oversized(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::EMSGSIZE)
}

pub fn send_eventd_log_record(
    socket_path: &str,
    record: &ServiceLogRecord,
) -> Result<EventdSendOutcome, BoundaryError> {
    LinuxEventdLogSink::new().send_eventd_log_records(socket_path, std::slice::from_ref(record))
}

#[cfg(test)]
mod tests;
