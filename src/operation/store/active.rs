use crate::ids::OperationId;
use crate::operation::OperationRecord;

use super::{OperationStore, OperationStoreError};

impl OperationStore {
    pub fn active_for_service(&self, service: &str) -> Vec<OperationId> {
        self.active_by_service
            .get(service)
            .cloned()
            .unwrap_or_default()
    }

    pub fn current_for_service(&self, service: &str) -> Option<&OperationRecord> {
        self.active_by_service
            .get(service)
            .into_iter()
            .flatten()
            .find_map(|id| self.records.get(id))
    }

    pub fn active_records(&self) -> Vec<&OperationRecord> {
        self.active_by_service
            .values()
            .flatten()
            .filter_map(|id| self.records.get(id))
            .collect()
    }

    pub(super) fn ensure_unused_id(&self, id: OperationId) -> Result<(), OperationStoreError> {
        if self.records.contains_key(&id) {
            Err(OperationStoreError::DuplicateOperationId { id })
        } else {
            Ok(())
        }
    }

    pub(super) fn insert_active(
        &mut self,
        service: String,
        id: OperationId,
        position: InsertPosition,
    ) {
        let active = self.active_by_service.entry(service).or_default();
        match position {
            InsertPosition::Front => active.insert(0, id),
            InsertPosition::Back => active.push(id),
        }
    }

    pub(super) fn remove_active(&mut self, service: &str, id: OperationId) {
        self.queued_behind.remove(&id);
        let Some(active) = self.active_by_service.get_mut(service) else {
            return;
        };
        active.retain(|active_id| *active_id != id);
        if active.is_empty() {
            self.active_by_service.remove(service);
        }
    }

    /// The operation `id` was queued behind, if the conflict table queued it
    /// (§8.3) and it has not been promoted yet.
    pub fn queued_behind(&self, id: OperationId) -> Option<OperationId> {
        self.queued_behind.get(&id).copied()
    }

    /// Whether `id` still waits for a live predecessor.
    pub fn is_queued_behind_live(&self, id: OperationId) -> bool {
        self.queued_behind(id).is_some_and(|behind| {
            self.records
                .get(&behind)
                .is_some_and(|operation| !operation.state.is_terminal())
        })
    }

    /// Queued operations whose predecessor has finished: still Pending, and
    /// now at the head of their service's active list. Each is promoted once,
    /// through `clear_queued_behind`.
    pub fn ready_queued_operations(&self) -> Vec<OperationRecord> {
        self.queued_behind
            .iter()
            .filter(|(id, behind)| {
                let predecessor_done = self
                    .records
                    .get(behind)
                    .is_none_or(|operation| operation.state.is_terminal());
                predecessor_done
                    && self
                        .records
                        .get(id)
                        .is_some_and(|operation| {
                            operation.state == crate::operation::OperationState::Pending
                                && self
                                    .current_for_service(&operation.service)
                                    .is_some_and(|head| head.id == operation.id)
                        })
            })
            .filter_map(|(id, _)| self.records.get(id).cloned())
            .collect()
    }

    pub fn clear_queued_behind(&mut self, id: OperationId) {
        self.queued_behind.remove(&id);
    }

    /// Queue `id` behind whatever is now at the head of its service's active
    /// list, if that is a different live operation.
    pub fn requeue_behind_head(&mut self, id: OperationId) {
        let Some(service) = self.records.get(&id).map(|record| record.service.clone()) else {
            return;
        };
        let head = self
            .active_for_service(&service)
            .into_iter()
            .find(|active| *active != id);
        if let Some(head) = head {
            self.queued_behind.insert(id, head);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::operation::store) enum InsertPosition {
    Front,
    Back,
}
