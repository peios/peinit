mod active;
mod create;
mod transition;

use std::collections::BTreeMap;

use crate::ids::JobId;

use super::model::{JobRecord, JobState, JobTransitionError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStoreError {
    DuplicateJobId {
        id: JobId,
    },
    UnknownJob {
        id: JobId,
    },
    InvalidInitialState {
        id: JobId,
        state: JobState,
    },
    ServiceMainAlreadyActive {
        service: String,
        active_job_id: JobId,
    },
    InvalidEventRecord {
        id: JobId,
        state: JobState,
        reason: &'static str,
    },
    Transition(JobTransitionError),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct JobStore {
    records: BTreeMap<JobId, JobRecord>,
    active_by_service: BTreeMap<String, Vec<JobId>>,
    /// Process descriptors whose job has left the store and that nothing
    /// references any more, waiting for the runtime to close them.
    ///
    /// The store is the pidfd's owner from `start` onwards, but it cannot close
    /// the descriptor itself: the supervisor works on a *clone* of the store
    /// and commits it only once the whole transition has succeeded, so a close
    /// inside the clone would take the descriptor away from the original if
    /// the transition failed. Queueing the release keeps it transactional --
    /// it only reaches the runtime once the finishing transition is committed
    /// -- and keeps the store free of descriptor I/O, so tests can hand it any
    /// number as a pidfd (PEI-816).
    released_pidfds: Vec<i32>,
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: JobId) -> Option<&JobRecord> {
        self.records.get(&id)
    }

    /// Descriptors released by every job that finished since the last take,
    /// on every terminal path, for the runtime to close. Each is handed out
    /// exactly once.
    pub fn take_released_pidfds(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.released_pidfds)
    }

    /// Mutate a stored record directly, to stage a state a correct caller
    /// would not — such as a current main job carrying a stale activation
    /// generation, which the ordinary invariants make unreachable.
    #[cfg(test)]
    pub(crate) fn record_mut(&mut self, id: JobId) -> Option<&mut JobRecord> {
        self.records.get_mut(&id)
    }
}
