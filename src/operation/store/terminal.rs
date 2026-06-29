use crate::ids::OperationId;

use super::{OperationEvent, OperationStore, OperationStoreError};

pub const DEFAULT_TERMINAL_OPERATION_RETENTION_NS: u64 = 60_000_000_000;

impl OperationStore {
    pub fn start_operation(
        &mut self,
        id: OperationId,
        started_at_ns: u64,
    ) -> Result<OperationEvent, OperationStoreError> {
        let record = self
            .records
            .get_mut(&id)
            .ok_or(OperationStoreError::UnknownOperation { id })?;
        record
            .start(started_at_ns)
            .map_err(OperationStoreError::Transition)?;
        Ok(OperationEvent::started(record))
    }

    pub fn complete_operation(
        &mut self,
        id: OperationId,
        completed_at_ns: u64,
        result: impl Into<String>,
    ) -> Result<OperationEvent, OperationStoreError> {
        let event = {
            let record = self
                .records
                .get_mut(&id)
                .ok_or(OperationStoreError::UnknownOperation { id })?;
            record
                .complete(completed_at_ns, result)
                .map_err(OperationStoreError::Transition)?;
            OperationEvent::completed(record)?
        };
        self.remove_active(&event.service, id);
        Ok(event)
    }

    pub fn fail_operation(
        &mut self,
        id: OperationId,
        completed_at_ns: u64,
        failure_reason: impl Into<String>,
    ) -> Result<OperationEvent, OperationStoreError> {
        let event = {
            let record = self
                .records
                .get_mut(&id)
                .ok_or(OperationStoreError::UnknownOperation { id })?;
            record
                .fail(completed_at_ns, failure_reason)
                .map_err(OperationStoreError::Transition)?;
            OperationEvent::failed(record)?
        };
        self.remove_active(&event.service, id);
        Ok(event)
    }

    pub fn cancel_operation(
        &mut self,
        id: OperationId,
        completed_at_ns: u64,
        reason: impl Into<String>,
    ) -> Result<OperationEvent, OperationStoreError> {
        let event = {
            let record = self
                .records
                .get_mut(&id)
                .ok_or(OperationStoreError::UnknownOperation { id })?;
            record
                .cancel(completed_at_ns, reason)
                .map_err(OperationStoreError::Transition)?;
            OperationEvent::cancelled(record)?
        };
        self.remove_active(&event.service, id);
        Ok(event)
    }

    pub fn purge_terminal_completed_at_or_before(&mut self, cutoff_ns: u64) -> Vec<OperationId> {
        let purged = self
            .records
            .iter()
            .filter_map(|(id, record)| {
                record
                    .state
                    .is_terminal()
                    .then_some(record.completed_at_ns)
                    .flatten()
                    .filter(|completed_at_ns| *completed_at_ns <= cutoff_ns)
                    .map(|_| *id)
            })
            .collect::<Vec<_>>();

        for id in &purged {
            self.records.remove(id);
        }
        purged
    }

    pub fn purge_terminal_retained_until(
        &mut self,
        now_ns: u64,
        retention_ns: u64,
    ) -> Vec<OperationId> {
        let Some(cutoff_ns) = now_ns.checked_sub(retention_ns) else {
            return Vec::new();
        };
        self.purge_terminal_completed_at_or_before(cutoff_ns)
    }

    pub fn next_terminal_retention_deadline_ns(&self, retention_ns: u64) -> Option<u64> {
        self.records
            .values()
            .filter_map(|record| {
                record
                    .state
                    .is_terminal()
                    .then_some(record.completed_at_ns)
                    .flatten()
                    .map(|completed_at_ns| completed_at_ns.saturating_add(retention_ns))
            })
            .min()
    }
}
