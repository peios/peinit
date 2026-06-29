use super::super::{JobEvent, JobRecord, JobState, JobType};
use super::{JobStore, JobStoreError};

impl JobStore {
    pub fn create_job(&mut self, job: JobRecord) -> Result<JobEvent, JobStoreError> {
        self.ensure_can_create(&job)?;
        let event = JobEvent::created(&job);
        let id = job.id;
        let service = job.service.clone();
        self.records.insert(id, job);
        if let Some(service) = service {
            self.insert_active(service, id);
        }
        Ok(event)
    }

    fn ensure_can_create(&self, job: &JobRecord) -> Result<(), JobStoreError> {
        if self.records.contains_key(&job.id) {
            return Err(JobStoreError::DuplicateJobId { id: job.id });
        }
        if job.state != JobState::Created {
            return Err(JobStoreError::InvalidInitialState {
                id: job.id,
                state: job.state,
            });
        }
        if job.job_type == JobType::ServiceMain
            && let Some(service) = &job.service
            && let Some(active_job_id) = self.current_service_main_job(service)
        {
            return Err(JobStoreError::ServiceMainAlreadyActive {
                service: service.clone(),
                active_job_id,
            });
        }
        Ok(())
    }
}
