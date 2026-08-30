use std::os::unix::net::UnixDatagram;

use crate::logging::{ServiceLogRecord, encode_eventd_log_records_into};

use super::BoundaryError;

pub trait EventdLogSink {
    fn send_eventd_log_records(
        &mut self,
        socket_path: &str,
        records: &[ServiceLogRecord],
    ) -> Result<(), BoundaryError>;
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
    ) -> Result<(), BoundaryError> {
        if records.is_empty() {
            return Ok(());
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
            Err(error) => {
                self.connected = None;
                return Err(BoundaryError::EventdLog(format!(
                    "send to {socket_path}: {error}"
                )));
            }
        };
        if sent == self.payload.len() {
            Ok(())
        } else {
            self.connected = None;
            Err(BoundaryError::EventdLog(format!(
                "partial datagram send to {socket_path}: {sent}/{} bytes",
                self.payload.len()
            )))
        }
    }
}

pub fn send_eventd_log_record(
    socket_path: &str,
    record: &ServiceLogRecord,
) -> Result<(), BoundaryError> {
    LinuxEventdLogSink::new().send_eventd_log_records(socket_path, std::slice::from_ref(record))
}

#[cfg(test)]
mod tests;
