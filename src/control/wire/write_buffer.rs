#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlWriteBuffer {
    bytes: Vec<u8>,
    written: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlWriteBufferError {
    AdvancePastEnd { written: usize, pending: usize },
}

impl ControlWriteBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&mut self, bytes: &[u8]) {
        self.compact_if_empty();
        self.bytes.extend_from_slice(bytes);
    }

    pub fn pending_slice(&self) -> &[u8] {
        &self.bytes[self.written..]
    }

    pub fn pending_len(&self) -> usize {
        self.pending_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending_len() == 0
    }

    pub fn advance_written(&mut self, count: usize) -> Result<(), ControlWriteBufferError> {
        let pending = self.pending_len();
        if count > pending {
            return Err(ControlWriteBufferError::AdvancePastEnd {
                written: count,
                pending,
            });
        }
        self.written += count;
        self.compact_if_empty();
        Ok(())
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
        self.written = 0;
    }

    fn compact_if_empty(&mut self) {
        if self.written == self.bytes.len() {
            self.bytes.clear();
            self.written = 0;
        }
    }
}

#[cfg(test)]
mod tests;
