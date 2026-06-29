use crate::ids::OperationId;

use super::super::StartExecutionStore;
use super::model::{PendingPreStartCheck, PendingPreStartCheckRegistration};

impl StartExecutionStore {
    pub fn record_pending_pre_start_check(
        &mut self,
        pending: PendingPreStartCheck,
    ) -> PendingPreStartCheckRegistration {
        let operation_id = pending.operation_id;
        let registration = PendingPreStartCheckRegistration {
            checks: pending.checks.clone(),
            helper_cgroup_id: pending.helper_cgroup_id(),
        };
        self.pending_pre_start_checks.insert(operation_id, pending);
        self.pending_pre_start_check_launches
            .push_back(operation_id);
        registration
    }

    pub fn pending_pre_start_check(
        &self,
        operation_id: OperationId,
    ) -> Option<&PendingPreStartCheck> {
        self.pending_pre_start_checks.get(&operation_id)
    }

    pub fn remove_pending_pre_start_check(
        &mut self,
        operation_id: OperationId,
    ) -> Option<PendingPreStartCheck> {
        self.pending_pre_start_checks.remove(&operation_id)
    }

    pub fn pop_pending_pre_start_check_launch(&mut self) -> Option<PendingPreStartCheck> {
        while let Some(operation_id) = self.pending_pre_start_check_launches.pop_front() {
            if let Some(pending) = self.pending_pre_start_checks.get(&operation_id) {
                return Some(pending.clone());
            }
        }
        None
    }

    pub fn pending_pre_start_check_launches(&self) -> Vec<OperationId> {
        self.pending_pre_start_check_launches
            .iter()
            .copied()
            .collect()
    }
}
