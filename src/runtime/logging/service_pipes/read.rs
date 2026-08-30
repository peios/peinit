use crate::boundary::{EventdLogSink, RealtimeClock};
use crate::logging::{ServiceLogRecord, eventd_log_batch_prefix_len};
use crate::runtime::{RuntimeEventRegistrar, RuntimeLogPipeTurn};

use super::RuntimeServiceLogPipes;

impl RuntimeServiceLogPipes {
    pub fn process_pipe_event<C, R>(
        &mut self,
        fd: i32,
        clock: &mut C,
        registrar: &mut R,
    ) -> RuntimeLogPipeTurn
    where
        C: RealtimeClock + ?Sized,
        R: RuntimeEventRegistrar + ?Sized,
    {
        let mut sink = std::mem::take(&mut self.eventd_sink);
        let turn = self.process_pipe_event_with_sink(fd, clock, registrar, &mut sink);
        self.eventd_sink = sink;
        turn
    }

    pub(in crate::runtime::logging) fn process_pipe_event_with_sink<C, R, S>(
        &mut self,
        fd: i32,
        clock: &mut C,
        registrar: &mut R,
        sink: &mut S,
    ) -> RuntimeLogPipeTurn
    where
        C: RealtimeClock + ?Sized,
        R: RuntimeEventRegistrar + ?Sized,
        S: EventdLogSink + ?Sized,
    {
        let Some(pipe) = self.pipes.get_mut(&fd) else {
            return RuntimeLogPipeTurn::Stale { fd };
        };
        let timestamp_ns = clock.realtime_ns().unwrap_or(0);
        let read = pipe.read_available(timestamp_ns, self.config.read_bytes_per_event);
        let job_id = pipe.job_id();
        self.forward_or_buffer_records(&read.records, sink);
        let output_dropped = job_id.and_then(|job_id| self.tee_to_sink(job_id, &read.records));
        if read.closed {
            let _ = registrar.unregister_source(fd);
            self.pipes.remove(&fd);
            if let Some(job_id) = job_id {
                self.release_sink_pipe(job_id);
            }
        }
        RuntimeLogPipeTurn::Read {
            fd,
            records: read.records,
            closed: read.closed,
            would_block: read.would_block,
            buffered_records: self.pre_eventd.records().len(),
            output_dropped,
        }
    }

    /// Write each line to the job's sink, dropping for the sink alone when it
    /// would block. Returns the job whose sink dropped for the first time.
    fn tee_to_sink(
        &mut self,
        job_id: crate::ids::JobId,
        records: &[ServiceLogRecord],
    ) -> Option<crate::ids::JobId> {
        let sink = self.sinks.get_mut(&job_id)?;
        let mut first_drop = false;
        for record in records {
            let mut line = record.message.clone().into_bytes();
            line.push(b'\n');
            match write_whole(&sink.fd, &line) {
                SinkWrite::Written => {}
                SinkWrite::WouldBlock => {
                    sink.dropped += 1;
                    if !sink.drop_reported {
                        sink.drop_reported = true;
                        first_drop = true;
                    }
                }
                SinkWrite::Failed => {
                    // Anything but would-block: the sink is gone.
                    self.sinks.remove(&job_id);
                    return first_drop.then_some(job_id);
                }
            }
        }
        first_drop.then_some(job_id)
    }

    fn release_sink_pipe(&mut self, job_id: crate::ids::JobId) {
        let close = match self.sinks.get_mut(&job_id) {
            Some(sink) => {
                sink.open_pipes = sink.open_pipes.saturating_sub(1);
                sink.open_pipes == 0
            }
            None => false,
        };
        if close {
            self.sinks.remove(&job_id);
        }
    }

    fn forward_or_buffer_records<S>(&mut self, records: &[ServiceLogRecord], sink: &mut S)
    where
        S: EventdLogSink + ?Sized,
    {
        let Some(socket_path) = self.eventd_socket_path.take() else {
            for record in records {
                self.pre_eventd.push(record.clone());
            }
            return;
        };

        let mut index = 0usize;
        while index < records.len() {
            let batch_len = eventd_log_batch_prefix_len(
                records[index..].iter(),
                self.config.eventd_log_datagram_bytes,
            );
            if batch_len == 0
                || sink
                    .send_eventd_log_records(&socket_path, &records[index..index + batch_len])
                    .is_err()
            {
                for unsent in &records[index..] {
                    self.pre_eventd.push(unsent.clone());
                }
                return;
            }
            index += batch_len;
        }
        self.eventd_socket_path = Some(socket_path);
    }
}

enum SinkWrite {
    Written,
    WouldBlock,
    Failed,
}

/// A whole-line write on a non-blocking descriptor: all of it, or none of
/// it counted, so a partial line never reaches the submitter.
fn write_whole(fd: &std::os::fd::OwnedFd, line: &[u8]) -> SinkWrite {
    use std::os::fd::AsRawFd;

    let mut written = 0usize;
    while written < line.len() {
        let rc = unsafe {
            libc::write(
                fd.as_raw_fd(),
                line[written..].as_ptr().cast(),
                line.len() - written,
            )
        };
        if rc < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if error.raw_os_error() == Some(libc::EAGAIN) {
                return SinkWrite::WouldBlock;
            }
            return SinkWrite::Failed;
        }
        written += rc as usize;
    }
    SinkWrite::Written
}
