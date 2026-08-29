use crate::ids::JobId;
use crate::job::JobState;
use crate::submitted::{JobView, SubmittedJobEntry, job_view};

use crate::supervisor::state::Supervisor;

impl Supervisor {
    /// The view of one submitted job, or `None` for an identifier the
    /// manager no longer holds.
    pub fn submitted_job_view(&self, job_id: JobId) -> Option<JobView> {
        let entry = self.submitted.get(job_id)?;
        job_view(entry, self.jobs.get(job_id))
    }

    /// The state a submitted job would be listed under.
    pub(in crate::supervisor) fn submitted_job_state(&self, entry: &SubmittedJobEntry) -> JobState {
        entry
            .outcome
            .as_ref()
            .map(|outcome| outcome.state)
            .or_else(|| self.jobs.get(entry.job_id).map(|record| record.state))
            .unwrap_or(JobState::Failed)
    }

    /// Whether a submit has been answered: the job has left `created`.
    pub(in crate::supervisor) fn submitted_job_left_created(&self, job_id: JobId) -> Option<bool> {
        let entry = self.submitted.get(job_id)?;
        if entry.outcome.is_some() {
            return Some(true);
        }
        Some(
            self.jobs
                .get(job_id)
                .is_some_and(|record| record.state != JobState::Created),
        )
    }
}
