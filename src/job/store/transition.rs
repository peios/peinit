use crate::ids::JobId;
use crate::security::TokenSummary;

use super::super::{JobEvent, JobExit, JobRecord, JobTransitionError, ProcessHandle};
use super::{JobStore, JobStoreError};

impl JobStore {
    pub fn start_job(
        &mut self,
        id: JobId,
        process: ProcessHandle,
        started_at_ns: u64,
    ) -> Result<JobEvent, JobStoreError> {
        let summary = self
            .records
            .get(&id)
            .ok_or(JobStoreError::UnknownJob { id })?
            .token_summary
            .clone();
        self.start_job_with_token_summary(id, process, summary, started_at_ns)
    }

    pub fn start_job_with_token_summary(
        &mut self,
        id: JobId,
        process: ProcessHandle,
        token_summary: TokenSummary,
        started_at_ns: u64,
    ) -> Result<JobEvent, JobStoreError> {
        let record = self
            .records
            .get_mut(&id)
            .ok_or(JobStoreError::UnknownJob { id })?;
        record.token_summary = token_summary;
        record
            .start(process, started_at_ns)
            .map_err(JobStoreError::Transition)?;
        JobEvent::started(record)
    }

    pub fn complete_job(
        &mut self,
        id: JobId,
        ended_at_ns: u64,
        exit_code: i32,
    ) -> Result<JobEvent, JobStoreError> {
        self.finish_job(id, |record| record.complete(ended_at_ns, exit_code))
    }

    pub fn fail_job_before_start(
        &mut self,
        id: JobId,
        ended_at_ns: u64,
        failure_cause: impl Into<String>,
    ) -> Result<JobEvent, JobStoreError> {
        let failure_cause = failure_cause.into();
        self.finish_job(id, |record| {
            record.fail_before_start(ended_at_ns, failure_cause)
        })
    }

    pub fn fail_running_job(
        &mut self,
        id: JobId,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
    ) -> Result<JobEvent, JobStoreError> {
        let failure_cause = failure_cause.into();
        self.finish_job(id, |record| {
            record.fail_running(ended_at_ns, exit, failure_cause)
        })
    }

    pub fn abandon_job(
        &mut self,
        id: JobId,
        ended_at_ns: u64,
        failure_cause: impl Into<String>,
    ) -> Result<JobEvent, JobStoreError> {
        let failure_cause = failure_cause.into();
        self.finish_job(id, |record| record.abandon(ended_at_ns, failure_cause))
    }

    fn finish_job<F>(&mut self, id: JobId, finish: F) -> Result<JobEvent, JobStoreError>
    where
        F: FnOnce(&mut JobRecord) -> Result<(), JobTransitionError>,
    {
        let event = {
            let record = self
                .records
                .get_mut(&id)
                .ok_or(JobStoreError::UnknownJob { id })?;
            finish(record).map_err(JobStoreError::Transition)?;
            JobEvent::ended(record)?
        };
        self.records.remove(&id);
        if let Some(service) = &event.service {
            self.remove_active(service, id);
        }
        Ok(event)
    }
}
