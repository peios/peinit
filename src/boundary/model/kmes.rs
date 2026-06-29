use super::BoundaryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KmesEvent {
    pub event_type: String,
    pub payload: Vec<u8>,
}

impl KmesEvent {
    pub fn new(event_type: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            event_type: event_type.into(),
            payload,
        }
    }
}

pub trait KmesEventSink {
    fn emit_kmes_event(&mut self, event: &KmesEvent) -> Result<(), BoundaryError>;

    fn emit_kmes_events(&mut self, events: &[KmesEvent]) -> Result<(), BoundaryError> {
        for event in events {
            self.emit_kmes_event(event)?;
        }
        Ok(())
    }
}
