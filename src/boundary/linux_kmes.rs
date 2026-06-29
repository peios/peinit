use peios::event::{self, EmitEntry};

use super::{BoundaryError, KmesEvent, KmesEventSink};

#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxKmesEventSink;

impl LinuxKmesEventSink {
    pub fn new() -> Self {
        Self
    }
}

impl KmesEventSink for LinuxKmesEventSink {
    fn emit_kmes_event(&mut self, event: &KmesEvent) -> Result<(), BoundaryError> {
        event::emit(&event.event_type, &event.payload).map_err(kmes_error)
    }

    fn emit_kmes_events(&mut self, events: &[KmesEvent]) -> Result<(), BoundaryError> {
        if events.is_empty() {
            return Ok(());
        }

        let entries: Vec<_> = events
            .iter()
            .map(|event| EmitEntry {
                event_type: event.event_type.as_str(),
                payload: event.payload.as_slice(),
            })
            .collect();

        event::emit_batch(&entries)
            .and_then(|emitted| {
                if emitted == entries.len() {
                    Ok(())
                } else {
                    Err(peios::Error::from_raw_os_error(libc::EIO))
                }
            })
            .map_err(kmes_error)
    }
}

fn kmes_error(error: peios::Error) -> BoundaryError {
    BoundaryError::Kmes(error.to_string())
}
