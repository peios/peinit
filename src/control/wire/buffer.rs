use super::frame::control_frame_decision;
use super::model::ControlFrameDecision;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlConnectionBuffer {
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlBufferConsumeError {
    TooMany { requested: usize, available: usize },
}

impl ControlConnectionBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn frame_decision(&self, max_request_bytes: usize) -> ControlFrameDecision {
        control_frame_decision(&self.bytes, max_request_bytes)
    }

    pub fn consume(&mut self, count: usize) -> Result<(), ControlBufferConsumeError> {
        if count > self.bytes.len() {
            return Err(ControlBufferConsumeError::TooMany {
                requested: count,
                available: self.bytes.len(),
            });
        }
        self.bytes.drain(..count);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[cfg(test)]
mod tests;
