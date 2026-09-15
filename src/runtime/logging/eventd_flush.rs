use crate::boundary::{EventdLogSink, EventdSendOutcome};
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

        let mut flush = self.flush_to_eventd(socket_path, sink);
        // Live forwarding discards oversized records as it meets them; they
        // are reported with the flush so the console line covers both routes.
        flush.discarded_records = flush
            .discarded_records
            .saturating_add(std::mem::take(&mut self.eventd_oversized_records));
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
        let mut progress = FlushProgress::default();
        let ceiling = match self.eventd_batch_ceiling(socket_path, sink) {
            Ok(ceiling) => ceiling,
            Err(error) => return progress.report(self, Some(format!("{error:?}"))),
        };
        while !self.pre_eventd.is_empty() {
            let batch_len = eventd_log_batch_prefix_len(self.pre_eventd.iter(), ceiling);
            if batch_len == 0 {
                // The front record alone is over the ceiling. Reporting that
                // as a transport failure kept it at the front, and every
                // later record behind it, for the rest of the boot: give it
                // up and carry on (PEI-807).
                progress.attempted_records += 1;
                progress.discarded_records += 1;
                self.pre_eventd.discard_front(1);
                continue;
            }

            let batch = self
                .pre_eventd
                .iter()
                .take(batch_len)
                .cloned()
                .collect::<Vec<_>>();
            progress.attempted_records += batch_len;
            match sink.send_eventd_log_records(socket_path, &batch) {
                Ok(EventdSendOutcome::Sent) => {
                    progress.sent_records += batch_len;
                    self.pre_eventd.discard_front(batch_len);
                }
                // Replay is the one place where waiting beats dropping: these
                // records are already buffered, the buffer is bounded, and the
                // next turn will try again. Crucially this is not an error, so
                // the socket path is not cleared and forwarding stays on
                // (PEI-357).
                Ok(EventdSendOutcome::Dropped) => return progress.report(self, None),
                // The socket refused the batch as too large although it was
                // built to the socket's own ceiling. Retrying it reproduces
                // the refusal exactly, so the batch is discarded rather than
                // kept: an identical replay every turn is what stopped log
                // delivery machine-wide (PEI-807).
                Ok(EventdSendOutcome::Oversized) => {
                    progress.discarded_records += batch_len;
                    self.pre_eventd.discard_front(batch_len);
                }
                Err(error) => return progress.report(self, Some(format!("{error:?}"))),
            }
        }
        progress.report(self, None)
    }
}

#[derive(Default)]
struct FlushProgress {
    attempted_records: usize,
    sent_records: usize,
    discarded_records: usize,
}

impl FlushProgress {
    fn report(
        &self,
        pipes: &RuntimeServiceLogPipes,
        error: Option<String>,
    ) -> RuntimeEventdLogFlush {
        RuntimeEventdLogFlush {
            eventd_active: true,
            socket_path_configured: true,
            attempted_records: self.attempted_records,
            sent_records: self.sent_records,
            buffered_records: pipes.pre_eventd.len(),
            discarded_records: self.discarded_records,
            error,
        }
    }
}
