use crate::ids::JobId;

use super::super::JobType;
use super::JobStore;

impl JobStore {
    pub fn active_for_service(&self, service: &str) -> Vec<JobId> {
        self.active_by_service
            .get(service)
            .cloned()
            .unwrap_or_default()
    }

    pub fn current_service_main_job(&self, service: &str) -> Option<JobId> {
        self.active_by_service
            .get(service)
            .into_iter()
            .flatten()
            .copied()
            .find(|id| {
                self.records
                    .get(id)
                    .is_some_and(|record| record.job_type == JobType::ServiceMain)
            })
    }

    pub fn active_job_by_pid(&self, pid: u32) -> Option<JobId> {
        self.records
            .iter()
            .find_map(|(id, record)| (record.pid == Some(pid)).then_some(*id))
    }

    pub(super) fn insert_active(&mut self, service: String, id: JobId) {
        self.active_by_service.entry(service).or_default().push(id);
    }

    pub(super) fn remove_active(&mut self, service: &str, id: JobId) {
        let Some(active) = self.active_by_service.get_mut(service) else {
            return;
        };
        active.retain(|active_id| *active_id != id);
        if active.is_empty() {
            self.active_by_service.remove(service);
        }
    }
}
