use crate::boundary::{EventdLogSink, LinuxEventdLogSink, RealtimeClock};
use crate::logging::ServiceLogRecord;
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
        let mut sink = LinuxEventdLogSink::new();
        self.process_pipe_event_with_sink(fd, clock, registrar, &mut sink)
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
        self.forward_or_buffer_records(&read.records, sink);
        if read.closed {
            let _ = registrar.unregister_source(fd);
            self.pipes.remove(&fd);
        }
        RuntimeLogPipeTurn::Read {
            fd,
            records: read.records,
            closed: read.closed,
            would_block: read.would_block,
            buffered_records: self.pre_eventd.records().len(),
        }
    }

    fn forward_or_buffer_records<S>(&mut self, records: &[ServiceLogRecord], sink: &mut S)
    where
        S: EventdLogSink + ?Sized,
    {
        let Some(socket_path) = self.eventd_socket_path.clone() else {
            for record in records {
                self.pre_eventd.push(record.clone());
            }
            return;
        };

        for (index, record) in records.iter().enumerate() {
            if sink.send_eventd_log_record(&socket_path, record).is_ok() {
                continue;
            }
            self.eventd_socket_path = None;
            for unsent in &records[index..] {
                self.pre_eventd.push(unsent.clone());
            }
            break;
        }
    }
}
