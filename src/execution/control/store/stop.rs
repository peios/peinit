use std::collections::BTreeMap;

use crate::ids::OperationId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopTimeoutDeadline {
    pub operation_id: OperationId,
    pub service: String,
    pub cgroup_id: String,
    pub due_at_ns: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct StopTimeoutStore {
    deadlines: BTreeMap<OperationId, StopTimeoutDeadline>,
}

impl StopTimeoutStore {
    pub(super) fn record(&mut self, deadline: StopTimeoutDeadline) {
        self.deadlines.insert(deadline.operation_id, deadline);
    }

    pub(super) fn due(&self, now_ns: u64) -> Vec<StopTimeoutDeadline> {
        self.deadlines
            .values()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .cloned()
            .collect()
    }

    pub(super) fn remove(&mut self, operation_id: OperationId) -> Option<StopTimeoutDeadline> {
        self.deadlines.remove(&operation_id)
    }

    pub(super) fn get(&self, operation_id: OperationId) -> Option<StopTimeoutDeadline> {
        self.deadlines.get(&operation_id).cloned()
    }

    pub(super) fn for_service(&self, service: &str) -> Option<StopTimeoutDeadline> {
        self.deadlines
            .values()
            .filter(|deadline| deadline.service == service)
            .min_by_key(|deadline| deadline.due_at_ns)
            .cloned()
    }

    pub(super) fn extend(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<StopTimeoutDeadline> {
        let deadline = self.deadlines.get_mut(&operation_id)?;
        deadline.due_at_ns = due_at_ns;
        Some(deadline.clone())
    }

    pub(super) fn next(&self) -> Option<StopTimeoutDeadline> {
        self.deadlines
            .values()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
            .cloned()
    }
}
