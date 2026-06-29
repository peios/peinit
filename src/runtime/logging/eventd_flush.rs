use crate::boundary::{EventdLogSink, LinuxEventdLogSink};

use super::{RuntimeEventdLogFlush, RuntimeServiceLogPipes};

impl RuntimeServiceLogPipes {
    pub fn sync_eventd_forwarding(
        &mut self,
        eventd_active: bool,
        socket_path: Option<&str>,
    ) -> RuntimeEventdLogFlush {
        let mut sink = LinuxEventdLogSink::new();
        self.sync_eventd_forwarding_with_sink(eventd_active, socket_path, &mut sink)
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
                self.pre_eventd.records().len(),
            );
        }
        let Some(socket_path) = socket_path else {
            self.eventd_socket_path = None;
            return RuntimeEventdLogFlush::unavailable(
                true,
                false,
                self.pre_eventd.records().len(),
            );
        };

        let flush = self.flush_to_eventd(socket_path, sink);
        if flush.error.is_some() {
            self.eventd_socket_path = None;
        } else {
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
        while let Some(record) = self.pre_eventd.front().cloned() {
            attempted_records += 1;
            match sink.send_eventd_log_record(socket_path, &record) {
                Ok(()) => {
                    sent_records += 1;
                    let _ = self.pre_eventd.pop_front();
                }
                Err(error) => {
                    return RuntimeEventdLogFlush {
                        eventd_active: true,
                        socket_path_configured: true,
                        attempted_records,
                        sent_records,
                        buffered_records: self.pre_eventd.records().len(),
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
