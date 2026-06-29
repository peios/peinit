use crate::ids::OperationIdAllocator;

use super::{OperationConflictDecision, OperationConflictRejection, resolve_operation_conflict};
use crate::operation::{OperationRecord, OperationSource, OperationState, OperationType};

fn active(existing_type: OperationType, state: OperationState) -> OperationRecord {
    let id = OperationIdAllocator::new()
        .allocate_batch(1, 1_717_171_717_123_456_789)
        .expect("operation id")[0];
    let mut operation = OperationRecord::new(
        id,
        existing_type,
        "svc",
        OperationSource::Admin,
        None,
        1_000,
    );
    match state {
        OperationState::Pending => {}
        OperationState::Running => operation.start(1_001).expect("running operation"),
        _ => panic!("test helper only creates active operations"),
    }
    operation
}

#[test]
fn creates_new_operation_when_no_active_operation_exists() {
    assert_eq!(
        resolve_operation_conflict(None, OperationType::Start),
        OperationConflictDecision::CreateNew,
    );

    let mut completed = active(OperationType::Start, OperationState::Running);
    completed.complete(1_010, "active").expect("complete");
    assert_eq!(
        resolve_operation_conflict(Some(&completed), OperationType::Stop),
        OperationConflictDecision::CreateNew,
    );
}

#[test]
fn same_start_stop_and_reload_requests_merge() {
    for operation_type in [
        OperationType::Start,
        OperationType::Stop,
        OperationType::Reload,
    ] {
        let existing = active(operation_type, OperationState::Pending);

        assert_eq!(
            resolve_operation_conflict(Some(&existing), operation_type),
            OperationConflictDecision::MergeIntoExisting {
                existing_id: existing.id,
            },
        );
    }
}

#[test]
fn restart_queues_behind_restart_but_start_merges_into_restart() {
    let restart = active(OperationType::Restart, OperationState::Pending);

    assert_eq!(
        resolve_operation_conflict(Some(&restart), OperationType::Start),
        OperationConflictDecision::MergeIntoExisting {
            existing_id: restart.id,
        },
    );
    assert_eq!(
        resolve_operation_conflict(Some(&restart), OperationType::Restart),
        OperationConflictDecision::QueueNew,
    );
}

#[test]
fn stop_cancels_pending_start_or_restart() {
    for existing_type in [OperationType::Start, OperationType::Restart] {
        let existing = active(existing_type, OperationState::Pending);

        assert_eq!(
            resolve_operation_conflict(Some(&existing), OperationType::Stop),
            OperationConflictDecision::CancelExistingThenCreate {
                existing_id: existing.id,
            },
        );
    }
}

#[test]
fn stop_aborts_running_start_restart_or_reload() {
    for existing_type in [
        OperationType::Start,
        OperationType::Restart,
        OperationType::Reload,
    ] {
        let existing = active(existing_type, OperationState::Running);

        assert_eq!(
            resolve_operation_conflict(Some(&existing), OperationType::Stop),
            OperationConflictDecision::AbortExistingThenCreate {
                existing_id: existing.id,
            },
        );
    }
}

#[test]
fn start_or_restart_queues_behind_stop() {
    for requested_type in [OperationType::Start, OperationType::Restart] {
        let existing = active(OperationType::Stop, OperationState::Running);

        assert_eq!(
            resolve_operation_conflict(Some(&existing), requested_type),
            OperationConflictDecision::QueueNew,
        );
    }
}

#[test]
fn restart_cancels_pending_start_but_queues_behind_running_start() {
    let pending = active(OperationType::Start, OperationState::Pending);
    let running = active(OperationType::Start, OperationState::Running);

    assert_eq!(
        resolve_operation_conflict(Some(&pending), OperationType::Restart),
        OperationConflictDecision::CancelExistingThenQueue {
            existing_id: pending.id,
        },
    );
    assert_eq!(
        resolve_operation_conflict(Some(&running), OperationType::Restart),
        OperationConflictDecision::QueueNew,
    );
}

#[test]
fn restart_aborts_running_reload() {
    let reload = active(OperationType::Reload, OperationState::Running);

    assert_eq!(
        resolve_operation_conflict(Some(&reload), OperationType::Restart),
        OperationConflictDecision::AbortExistingThenCreate {
            existing_id: reload.id,
        },
    );
}

#[test]
fn stop_or_restart_cancels_pending_reload() {
    for requested_type in [OperationType::Stop, OperationType::Restart] {
        let reload = active(OperationType::Reload, OperationState::Pending);

        assert_eq!(
            resolve_operation_conflict(Some(&reload), requested_type),
            OperationConflictDecision::CancelExistingThenCreate {
                existing_id: reload.id,
            },
        );
    }
}

#[test]
fn reset_is_rejected_while_any_operation_is_active() {
    let existing = active(OperationType::Stop, OperationState::Pending);

    assert_eq!(
        resolve_operation_conflict(Some(&existing), OperationType::Reset),
        OperationConflictDecision::Reject(
            OperationConflictRejection::ResetWhileOperationInProgress
        ),
    );
}
