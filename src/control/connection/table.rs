use std::collections::BTreeMap;

use super::record::ControlConnectionRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlConnectionAdmissionDecision {
    Accept,
    RejectAtSocket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlConnectionAdmission<R> {
    Accepted {
        fd: i32,
        active_connections: usize,
    },
    RejectedAtSocket {
        fd: i32,
        record: R,
        active_connections: usize,
        max_connections: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlConnectionTableError {
    AlreadyTracked { fd: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlConnectionTable<R> {
    max_connections: usize,
    records: BTreeMap<i32, R>,
}

impl<R> ControlConnectionTable<R> {
    pub fn new(max_connections: usize) -> Self {
        Self {
            max_connections,
            records: BTreeMap::new(),
        }
    }

    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    pub fn set_max_connections(&mut self, max_connections: usize) {
        self.max_connections = max_connections;
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn admission_decision(&self) -> ControlConnectionAdmissionDecision {
        control_connection_admission_decision(self.records.len(), self.max_connections)
    }

    pub fn admit(
        &mut self,
        fd: i32,
        record: R,
    ) -> Result<ControlConnectionAdmission<R>, ControlConnectionTableError> {
        if self.records.contains_key(&fd) {
            return Err(ControlConnectionTableError::AlreadyTracked { fd });
        }
        if self.admission_decision() == ControlConnectionAdmissionDecision::RejectAtSocket {
            return Ok(ControlConnectionAdmission::RejectedAtSocket {
                fd,
                record,
                active_connections: self.records.len(),
                max_connections: self.max_connections,
            });
        }
        self.records.insert(fd, record);
        Ok(ControlConnectionAdmission::Accepted {
            fd,
            active_connections: self.records.len(),
        })
    }

    pub fn get(&self, fd: i32) -> Option<&R> {
        self.records.get(&fd)
    }

    pub fn get_mut(&mut self, fd: i32) -> Option<&mut R> {
        self.records.get_mut(&fd)
    }

    pub fn remove(&mut self, fd: i32) -> Option<R> {
        self.records.remove(&fd)
    }

    pub fn fds(&self) -> Vec<i32> {
        self.records.keys().copied().collect()
    }
}

impl<I> ControlConnectionTable<ControlConnectionRecord<I>> {
    pub fn has_pending_waits(&self) -> bool {
        self.records
            .values()
            .any(|record| record.state().pending_wait().is_some())
    }

    pub fn idle_fds(&self, now_ns: u64, timeout_secs: u64) -> Vec<i32> {
        self.records
            .iter()
            .filter_map(|(fd, record)| {
                record
                    .state()
                    .idle_timeout_expired(now_ns, timeout_secs)
                    .then_some(*fd)
            })
            .collect()
    }

    pub fn next_idle_deadline_ns(&self, timeout_secs: u64) -> Option<u64> {
        self.records
            .values()
            .filter_map(|record| record.state().idle_deadline_ns(timeout_secs))
            .min()
    }
}

pub fn control_connection_admission_decision(
    active_connections: usize,
    max_connections: usize,
) -> ControlConnectionAdmissionDecision {
    if active_connections >= max_connections {
        ControlConnectionAdmissionDecision::RejectAtSocket
    } else {
        ControlConnectionAdmissionDecision::Accept
    }
}
