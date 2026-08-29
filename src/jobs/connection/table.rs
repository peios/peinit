use crate::control::connection::ControlConnectionTable;

use super::record::JobsConnectionRecord;

/// The jobs connections share the control table's admission and bookkeeping;
/// only what counts as idle differs, and that is decided by the record.
pub type JobsConnectionTable<I> = ControlConnectionTable<JobsConnectionRecord<I>>;

impl<I> ControlConnectionTable<JobsConnectionRecord<I>> {
    pub fn has_pending_jobs_waits(&self) -> bool {
        self.records()
            .any(|record| record.state().pending_wait().is_some())
    }

    pub fn jobs_idle_fds(&self, now_ns: u64, timeout_secs: u64) -> Vec<i32> {
        self.entries()
            .filter_map(|(fd, record)| {
                record
                    .state()
                    .idle_timeout_expired(now_ns, timeout_secs)
                    .then_some(fd)
            })
            .collect()
    }

    pub fn next_jobs_idle_deadline_ns(&self, timeout_secs: u64) -> Option<u64> {
        self.records()
            .filter_map(|record| record.state().idle_deadline_ns(timeout_secs))
            .min()
    }
}
