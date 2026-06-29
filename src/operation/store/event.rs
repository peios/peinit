use crate::ids::OperationId;

use crate::operation::{
    OperationRecord, OperationSource, OperationState, OperationType, TokenSummary,
};

use super::OperationStoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationEvent {
    pub operation_id: OperationId,
    pub operation_type: OperationType,
    pub service: String,
    pub source: OperationSource,
    pub caller: Option<TokenSummary>,
    pub state: OperationState,
    pub detail: OperationEventDetail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationEventDetail {
    Requested,
    Started,
    Completed {
        duration_ns: u64,
        result: String,
    },
    Failed {
        duration_ns: u64,
        failure_reason: String,
    },
    Merged {
        merged_into: OperationId,
    },
    Cancelled {
        reason: String,
    },
    Aborted {
        duration_ns: u64,
        reason: String,
    },
}

impl OperationEvent {
    pub fn requested(record: &OperationRecord) -> Self {
        Self::new(record, OperationEventDetail::Requested)
    }

    pub fn started(record: &OperationRecord) -> Self {
        Self::new(record, OperationEventDetail::Started)
    }

    pub fn completed(record: &OperationRecord) -> Result<Self, OperationStoreError> {
        Ok(Self::new(
            record,
            OperationEventDetail::Completed {
                duration_ns: duration(record, "completed operation has duration")?,
                result: result(record, "completed operation has result")?,
            },
        ))
    }

    pub fn failed(record: &OperationRecord) -> Result<Self, OperationStoreError> {
        Ok(Self::new(
            record,
            OperationEventDetail::Failed {
                duration_ns: duration(record, "failed operation has duration")?,
                failure_reason: result(record, "failed operation has failure reason")?,
            },
        ))
    }

    pub fn merged(record: &OperationRecord) -> Result<Self, OperationStoreError> {
        Ok(Self::new(
            record,
            OperationEventDetail::Merged {
                merged_into: record
                    .merged_into
                    .ok_or_else(|| invalid_record(record, "merged operation records target"))?,
            },
        ))
    }

    pub fn cancelled(record: &OperationRecord) -> Result<Self, OperationStoreError> {
        Ok(Self::new(
            record,
            OperationEventDetail::Cancelled {
                reason: result(record, "cancelled operation has reason")?,
            },
        ))
    }

    pub fn aborted(record: &OperationRecord) -> Result<Self, OperationStoreError> {
        Ok(Self::new(
            record,
            OperationEventDetail::Aborted {
                duration_ns: duration(record, "aborted operation has duration")?,
                reason: result(record, "aborted operation has reason")?,
            },
        ))
    }

    fn new(record: &OperationRecord, detail: OperationEventDetail) -> Self {
        Self {
            operation_id: record.id,
            operation_type: record.operation_type,
            service: record.service.clone(),
            source: record.source,
            caller: record.caller.clone(),
            state: record.state,
            detail,
        }
    }
}

fn duration(record: &OperationRecord, reason: &'static str) -> Result<u64, OperationStoreError> {
    record
        .duration_ns()
        .ok_or_else(|| invalid_record(record, reason))
}

fn result(record: &OperationRecord, reason: &'static str) -> Result<String, OperationStoreError> {
    record
        .result
        .clone()
        .ok_or_else(|| invalid_record(record, reason))
}

fn invalid_record(record: &OperationRecord, reason: &'static str) -> OperationStoreError {
    OperationStoreError::InvalidEventRecord {
        id: record.id,
        state: record.state,
        reason,
    }
}
