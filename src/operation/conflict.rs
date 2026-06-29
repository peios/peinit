use crate::ids::OperationId;

use super::{OperationRecord, OperationState, OperationType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationConflictDecision {
    CreateNew,
    MergeIntoExisting { existing_id: OperationId },
    CancelExistingThenCreate { existing_id: OperationId },
    AbortExistingThenCreate { existing_id: OperationId },
    CancelExistingThenQueue { existing_id: OperationId },
    QueueNew,
    Reject(OperationConflictRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationConflictRejection {
    ResetWhileOperationInProgress,
    UnsupportedActiveConflict {
        existing_type: OperationType,
        existing_state: OperationState,
        requested_type: OperationType,
    },
}

pub fn resolve_operation_conflict(
    existing: Option<&OperationRecord>,
    requested_type: OperationType,
) -> OperationConflictDecision {
    let Some(existing) = existing else {
        return OperationConflictDecision::CreateNew;
    };
    if existing.state.is_terminal() {
        return OperationConflictDecision::CreateNew;
    }
    if requested_type == OperationType::Reset {
        return OperationConflictDecision::Reject(
            OperationConflictRejection::ResetWhileOperationInProgress,
        );
    }

    match (existing.operation_type, existing.state, requested_type) {
        (OperationType::Start, _, OperationType::Start)
        | (OperationType::Stop, _, OperationType::Stop)
        | (OperationType::Reload, _, OperationType::Reload)
        | (OperationType::Restart, _, OperationType::Start) => {
            OperationConflictDecision::MergeIntoExisting {
                existing_id: existing.id,
            }
        }
        (OperationType::Start, OperationState::Pending, OperationType::Stop)
        | (OperationType::Restart, OperationState::Pending, OperationType::Stop)
        | (OperationType::Reload, OperationState::Pending, OperationType::Stop)
        | (OperationType::Reload, OperationState::Pending, OperationType::Restart) => {
            OperationConflictDecision::CancelExistingThenCreate {
                existing_id: existing.id,
            }
        }
        (OperationType::Start, OperationState::Running, OperationType::Stop)
        | (OperationType::Restart, OperationState::Running, OperationType::Stop)
        | (OperationType::Reload, OperationState::Running, OperationType::Stop)
        | (OperationType::Reload, OperationState::Running, OperationType::Restart) => {
            OperationConflictDecision::AbortExistingThenCreate {
                existing_id: existing.id,
            }
        }
        (OperationType::Stop, _, OperationType::Start)
        | (OperationType::Stop, _, OperationType::Restart)
        | (OperationType::Restart, _, OperationType::Restart)
        | (OperationType::Start, OperationState::Running, OperationType::Restart) => {
            OperationConflictDecision::QueueNew
        }
        (OperationType::Start, OperationState::Pending, OperationType::Restart) => {
            OperationConflictDecision::CancelExistingThenQueue {
                existing_id: existing.id,
            }
        }
        _ => OperationConflictDecision::Reject(
            OperationConflictRejection::UnsupportedActiveConflict {
                existing_type: existing.operation_type,
                existing_state: existing.state,
                requested_type,
            },
        ),
    }
}

#[cfg(test)]
mod tests;
