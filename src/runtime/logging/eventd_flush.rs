use crate::boundary::EventdLogSink;
use crate::logging::eventd_log_batch_prefix_len;

use super::{RuntimeEventdLogFlush, RuntimeServiceLogPipes};

impl RuntimeServiceLogPipes {
    pub fn sync_eventd_forwarding(
        &mut self,
        eventd_active: bool,
        socket_path: Option<&str>,
    ) -> RuntimeEventdLogFlush {
        if !eventd_active || socket_path.is_none() || socket_path == Some("") {
            self.eventd_sink.disconnect();
        }
        let mut sink = std::mem::take(&mut self.eventd_sink);
        let flush = self.sync_eventd_forwarding_with_sink(eventd_active, socket_path, &mut sink);
        self.eventd_sink = sink;
        flush
    }

    pub fn sync_eventd_forwarding_with_sink<S>(
        &mut self,
        eventd_active: bool,
        socket_path: Option<&str>,
        sink: &mut S,
    ) -> RuntimeEventdLogFlush
    where
        S: EventdLogSink + ?Sized,
    {
        let socket_path = socket_path.filter(|path| !path.is_empty());
        if !eventd_active {
            self.eventd_socket_path = None;
            return RuntimeEventdLogFlush::unavailable(
                false,
                socket_path.is_some(),
                self.pre_eventd.len(),
            );
        }
        let Some(socket_path) = socket_path else {
            self.eventd_socket_path = None;
            return RuntimeEventdLogFlush::unavailable(true, false, self.pre_eventd.len());
        };

        let flush = self.flush_to_eventd(socket_path, sink);
        if flush.error.is_some() {
            self.eventd_socket_path = None;
        } else if self.eventd_socket_path.as_deref() != Some(socket_path) {
            self.eventd_socket_path = Some(socket_path.to_string());
        }
        flush
    }

    pub fn flush_to_eventd<S>(&mut self, socket_path: &str, sink: &mut S) -> RuntimeEventdLogFlush
    where
        S: EventdLogSink + ?Sized,
    {
        let mut attempted_records = 0;
        let mut sent_records = 0;
        while !self.pre_eventd.is_empty() {
            let batch_len = eventd_log_batch_prefix_len(
                self.pre_eventd.iter(),
                self.config.eventd_log_datagram_bytes,
            );
            if batch_len == 0 {
                attempted_records += 1;
                return RuntimeEventdLogFlush {
                    eventd_active: true,
                    socket_path_configured: true,
                    attempted_records,
                    sent_records,
                    buffered_records: self.pre_eventd.len(),
                    error: Some(format!(
                        "front log record exceeds eventd datagram ceiling of {} bytes",
                        self.config.eventd_log_datagram_bytes
                    )),
                };
            }

            let batch = self
                .pre_eventd
                .iter()
                .take(batch_len)
                .cloned()
                .collect::<Vec<_>>();
            attempted_records += batch_len;
            match sink.send_eventd_log_records(socket_path, &batch) {
                Ok(()) => {
                    sent_records += batch_len;
                    self.pre_eventd.discard_front(batch_len);
                }
                Err(error) => {
                    return RuntimeEventdLogFlush {
                        eventd_active: true,
                        socket_path_configured: true,
                        attempted_records,
                        sent_records,
                        buffered_records: self.pre_eventd.len(),
                        error: Some(format!("{error:?}")),
                    };
                }
            }
        }
        RuntimeEventdLogFlush {
            eventd_active: true,
            socket_path_configured: true,
            attempted_records,
            sent_records,
            buffered_records: 0,
            error: None,
        }
    }
}
