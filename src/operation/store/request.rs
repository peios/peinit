use crate::ids::OperationId;
use crate::operation::conflict::{OperationConflictDecision, resolve_operation_conflict};
use crate::operation::{OperationRecord, OperationSource, OperationType, TokenSummary};

use super::replacement::ExistingTermination;
use super::{InsertPosition, OperationEvent, OperationStore, OperationStoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRequest {
    pub id: OperationId,
    pub operation_type: OperationType,
    pub service: String,
    pub source: OperationSource,
    pub caller: Option<TokenSummary>,
    pub created_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRequestOutcome {
    pub returned_operation_id: OperationId,
    pub stored_operation_id: OperationId,
    pub decision: OperationConflictDecision,
    pub events: Vec<OperationEvent>,
}

impl OperationStore {
    pub fn request_operation(
        &mut self,
        request: OperationRequest,
    ) -> Result<OperationRequestOutcome, OperationStoreError> {
        self.ensure_unused_id(request.id)?;
        let decision = self.resolve_request(&request)?;
        match decision.clone() {
            OperationConflictDecision::CreateNew => {
                self.insert_requested(request, decision, InsertPosition::Back)
            }
            OperationConflictDecision::QueueNew => {
                self.insert_requested(request, decision, InsertPosition::Back)
            }
            OperationConflictDecision::MergeIntoExisting { existing_id } => {
                self.merge_request(request, decision, existing_id)
            }
            OperationConflictDecision::CancelExistingThenCreate { existing_id } => self
                .replace_existing(
                    request,
                    decision,
                    existing_id,
                    ExistingTermination::Cancel("superseded_by_later_operation"),
                    InsertPosition::Front,
                ),
            OperationConflictDecision::AbortExistingThenCreate { existing_id } => self
                .replace_existing(
                    request,
                    decision,
                    existing_id,
                    ExistingTermination::Abort("superseded_by_later_operation"),
                    InsertPosition::Front,
                ),
            OperationConflictDecision::CancelExistingThenQueue { existing_id } => self
                .replace_existing(
                    request,
                    decision,
                    existing_id,
                    ExistingTermination::Cancel("superseded_by_restart"),
                    InsertPosition::Back,
                ),
            OperationConflictDecision::Reject(rejection) => {
                Err(OperationStoreError::ConflictRejected(rejection))
            }
        }
    }

    fn resolve_request(
        &self,
        request: &OperationRequest,
    ) -> Result<OperationConflictDecision, OperationStoreError> {
        let active_ids = self.active_for_service(&request.service);
        for id in &active_ids {
            let existing =
                self.records
                    .get(id)
                    .ok_or_else(|| OperationStoreError::InvalidActiveIndex {
                        service: request.service.clone(),
                        id: *id,
                    })?;
            let decision = resolve_operation_conflict(Some(existing), request.operation_type);
            if matches!(
                decision,
                OperationConflictDecision::MergeIntoExisting { .. }
            ) {
                return Ok(decision);
            }
        }

        let existing = active_ids.first().and_then(|id| self.records.get(id));
        Ok(resolve_operation_conflict(existing, request.operation_type))
    }

    pub(super) fn insert_requested(
        &mut self,
        request: OperationRequest,
        decision: OperationConflictDecision,
        position: InsertPosition,
    ) -> Result<OperationRequestOutcome, OperationStoreError> {
        let record = record_from_request(request);
        let event = OperationEvent::requested(&record);
        let id = record.id;
        let service = record.service.clone();
        self.records.insert(id, record);
        self.insert_active(service, id, position);
        Ok(OperationRequestOutcome {
            returned_operation_id: id,
            stored_operation_id: id,
            decision,
            events: vec![event],
        })
    }

    fn merge_request(
        &mut self,
        request: OperationRequest,
        decision: OperationConflictDecision,
        existing_id: OperationId,
    ) -> Result<OperationRequestOutcome, OperationStoreError> {
        let mut record = record_from_request(request);
        let requested = OperationEvent::requested(&record);
        record
            .merge_into(existing_id, record.created_at_ns)
            .map_err(OperationStoreError::Transition)?;
        let merged = OperationEvent::merged(&record)?;
        let id = record.id;
        self.records.insert(id, record);
        Ok(OperationRequestOutcome {
            returned_operation_id: existing_id,
            stored_operation_id: id,
            decision,
            events: vec![requested, merged],
        })
    }
}

fn record_from_request(request: OperationRequest) -> OperationRecord {
    OperationRecord::new(
        request.id,
        request.operation_type,
        request.service,
        request.source,
        request.caller,
        request.created_at_ns,
    )
}
