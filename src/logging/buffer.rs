use std::collections::VecDeque;

use super::ServiceLogRecord;

pub const DEFAULT_PRE_EVENTD_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreEventdLogBuffer {
    capacity_bytes: usize,
    used_bytes: usize,
    records: VecDeque<BufferedLogRecord>,
}

impl PreEventdLogBuffer {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity_bytes,
            used_bytes: 0,
            records: VecDeque::new(),
        }
    }

    pub fn push(&mut self, record: ServiceLogRecord) {
        let size_bytes = estimated_record_bytes(&record);
        if size_bytes > self.capacity_bytes {
            return;
        }
        while self.used_bytes + size_bytes > self.capacity_bytes {
            let Some(removed) = self.records.pop_front() else {
                break;
            };
            self.used_bytes = self.used_bytes.saturating_sub(removed.size_bytes);
        }
        self.used_bytes += size_bytes;
        self.records
            .push_back(BufferedLogRecord { record, size_bytes });
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.used_bytes = 0;
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    pub fn records(&self) -> Vec<ServiceLogRecord> {
        self.records
            .iter()
            .map(|entry| entry.record.clone())
            .collect()
    }

    pub fn front(&self) -> Option<&ServiceLogRecord> {
        self.records.front().map(|entry| &entry.record)
    }

    pub fn pop_front(&mut self) -> Option<ServiceLogRecord> {
        let removed = self.records.pop_front()?;
        self.used_bytes = self.used_bytes.saturating_sub(removed.size_bytes);
        Some(removed.record)
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

impl Default for PreEventdLogBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_PRE_EVENTD_BUFFER_BYTES)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BufferedLogRecord {
    record: ServiceLogRecord,
    size_bytes: usize,
}

fn estimated_record_bytes(record: &ServiceLogRecord) -> usize {
    record.origin.len()
        + record.message.len()
        + std::mem::size_of::<u64>()
        + record.job_id.map(|_| 16).unwrap_or(0)
        + 32
}

#[cfg(test)]
mod tests;
