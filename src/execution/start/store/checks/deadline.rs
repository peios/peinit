use crate::ids::OperationId;

use super::super::StartExecutionStore;
use super::model::PreStartCheckDeadline;

impl StartExecutionStore {
    pub fn remove_pre_start_check_deadline(
        &mut self,
        operation_id: OperationId,
    ) -> Option<PreStartCheckDeadline> {
        self.pre_start_check_deadlines.remove(&operation_id)
    }

    pub fn due_pre_start_check_deadlines(&self, now_ns: u64) -> Vec<PreStartCheckDeadline> {
        self.pre_start_check_deadlines
            .values()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .cloned()
            .collect()
    }

    pub fn next_pre_start_check_timeout(&self) -> Option<PreStartCheckDeadline> {
        self.pre_start_check_deadlines
            .values()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
            .cloned()
    }
}
