use crate::control::wire::{ControlConnectionBuffer, ControlWriteBuffer};
use crate::ids::OperationId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlOperationWait {
    pub operation_id: OperationId,
    pub service: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlConnectionState {
    read_buffer: ControlConnectionBuffer,
    write_buffer: ControlWriteBuffer,
    close_after_write: bool,
    pending_wait: Option<ControlOperationWait>,
    last_activity_ns: Option<u64>,
}

impl ControlConnectionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_buffer(&self) -> &ControlConnectionBuffer {
        &self.read_buffer
    }

    pub fn read_buffer_mut(&mut self) -> &mut ControlConnectionBuffer {
        &mut self.read_buffer
    }

    pub fn write_buffer(&self) -> &ControlWriteBuffer {
        &self.write_buffer
    }

    pub fn write_buffer_mut(&mut self) -> &mut ControlWriteBuffer {
        &mut self.write_buffer
    }

    pub fn enqueue_response(&mut self, response_line: &[u8], close_after_response: bool) {
        self.write_buffer.enqueue(response_line);
        self.close_after_write |= close_after_response;
    }

    pub fn pending_write_bytes(&self) -> usize {
        self.write_buffer.pending_len()
    }

    pub fn close_after_write(&self) -> bool {
        self.close_after_write
    }

    pub fn mark_close_after_write(&mut self) {
        self.close_after_write = true;
    }

    pub fn pending_wait(&self) -> Option<&ControlOperationWait> {
        self.pending_wait.as_ref()
    }

    pub fn set_pending_wait(&mut self, wait: ControlOperationWait) {
        self.pending_wait = Some(wait);
    }

    pub fn clear_pending_wait(&mut self) -> Option<ControlOperationWait> {
        self.pending_wait.take()
    }

    pub fn mark_activity(&mut self, observed_at_ns: u64) {
        self.last_activity_ns = Some(observed_at_ns);
    }

    pub fn last_activity_ns(&self) -> Option<u64> {
        self.last_activity_ns
    }

    pub fn idle_deadline_ns(&self, timeout_secs: u64) -> Option<u64> {
        if self.pending_wait.is_some() || !self.write_buffer.is_empty() {
            return None;
        }
        self.last_activity_ns.map(|last_activity_ns| {
            last_activity_ns.saturating_add(timeout_secs.saturating_mul(1_000_000_000))
        })
    }

    pub fn idle_timeout_expired(&self, now_ns: u64, timeout_secs: u64) -> bool {
        self.idle_deadline_ns(timeout_secs)
            .is_some_and(|deadline_ns| now_ns >= deadline_ns)
    }
}
