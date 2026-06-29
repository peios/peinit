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
        let Some(active) = self.active_by_service.get_mut(service) else {
            return;
        };
        active.retain(|active_id| *active_id != id);
        if active.is_empty() {
            self.active_by_service.remove(service);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::operation::store) enum InsertPosition {
    Front,
    Back,
}
