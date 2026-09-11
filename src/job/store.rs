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
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: JobId) -> Option<&JobRecord> {
        self.records.get(&id)
    }

    /// Mutate a stored record directly, to stage a state a correct caller
    /// would not — such as a current main job carrying a stale activation
    /// generation, which the ordinary invariants make unreachable.
    #[cfg(test)]
    pub(crate) fn record_mut(&mut self, id: JobId) -> Option<&mut JobRecord> {
        self.records.get_mut(&id)
    }
}
