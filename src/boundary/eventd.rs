use std::os::unix::net::UnixDatagram;

use crate::logging::{ServiceLogRecord, encode_eventd_log_record};

use super::BoundaryError;

pub trait EventdLogSink {
    fn send_eventd_log_record(
        &mut self,
        socket_path: &str,
        record: &ServiceLogRecord,
    ) -> Result<(), BoundaryError>;
}

#[derive(Debug, Default)]
pub struct LinuxEventdLogSink;

impl LinuxEventdLogSink {
    pub fn new() -> Self {
        Self
    }
}

impl EventdLogSink for LinuxEventdLogSink {
    fn send_eventd_log_record(
        &mut self,
        socket_path: &str,
        record: &ServiceLogRecord,
    ) -> Result<(), BoundaryError> {
        send_eventd_log_record(socket_path, record)
    }
}

pub fn send_eventd_log_record(
    socket_path: &str,
    record: &ServiceLogRecord,
) -> Result<(), BoundaryError> {
    let payload = encode_eventd_log_record(record);
    let socket = UnixDatagram::unbound()
        .map_err(|error| BoundaryError::EventdLog(format!("create datagram socket: {error}")))?;
    socket
        .set_nonblocking(true)
        .map_err(|error| BoundaryError::EventdLog(format!("set nonblocking: {error}")))?;
    let sent = socket
        .send_to(&payload, socket_path)
        .map_err(|error| BoundaryError::EventdLog(format!("send to {socket_path}: {error}")))?;
    if sent == payload.len() {
        Ok(())
    } else {
        Err(BoundaryError::EventdLog(format!(
            "partial datagram send to {socket_path}: {sent}/{} bytes",
            payload.len()
        )))
    }
}

#[cfg(test)]
mod tests;
