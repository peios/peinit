use crate::ids::{JobId, OperationId};

use super::StartExecutionStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessDeadline {
    pub operation_id: OperationId,
    pub job_id: JobId,
    pub service: String,
    pub service_cgroup_id: String,
    pub due_at_ns: u64,
}

impl StartExecutionStore {
    pub fn record_readiness_deadline(&mut self, deadline: ReadinessDeadline) {
        self.readiness_deadlines
            .insert(deadline.operation_id, deadline);
    }

    pub fn remove_readiness_deadline(
        &mut self,
        operation_id: OperationId,
    ) -> Option<ReadinessDeadline> {
        self.readiness_deadlines.remove(&operation_id)
    }

    pub fn readiness_deadline(&self, operation_id: OperationId) -> Option<ReadinessDeadline> {
        self.readiness_deadlines.get(&operation_id).cloned()
    }

    pub fn readiness_deadline_for_service(&self, service: &str) -> Option<ReadinessDeadline> {
        self.readiness_deadlines
            .values()
            .filter(|deadline| deadline.service == service)
            .min_by_key(|deadline| deadline.due_at_ns)
            .cloned()
    }

    pub fn extend_readiness_deadline(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<ReadinessDeadline> {
        let deadline = self.readiness_deadlines.get_mut(&operation_id)?;
        deadline.due_at_ns = due_at_ns;
        Some(deadline.clone())
    }

    pub fn due_readiness_deadlines(&self, now_ns: u64) -> Vec<ReadinessDeadline> {
        self.readiness_deadlines
            .values()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .cloned()
            .collect()
    }

    pub fn next_readiness_timeout(&self) -> Option<ReadinessDeadline> {
        self.readiness_deadlines
            .values()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
            .cloned()
    }
}
