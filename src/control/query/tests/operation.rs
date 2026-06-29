use crate::control::query::{QueryError, operation_status};
use crate::operation::OperationState;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::DEFAULT_TERMINAL_OPERATION_RETENTION_NS;
use crate::operation::store::OperationStore;
use crate::operation::{OperationSource, OperationType};

use super::{operation_ids, operation_request};

#[test]
fn operation_status_projects_running_operation() {
    let ids = operation_ids(1);
    let mut operations = OperationStore::new();
    operations
        .request_operation(operation_request(
            ids[0],
            OperationType::Start,
            "svc",
            1_000,
        ))
        .expect("request");
    operations.start_operation(ids[0], 1_010).expect("start");

    let status = operation_status(&operations, ids[0]).expect("operation status");

    assert_eq!(status.id, ids[0]);
    assert_eq!(status.operation_type, OperationType::Start);
    assert_eq!(status.service, "svc");
    assert_eq!(status.source, OperationSource::Admin);
    assert_eq!(status.state, OperationState::Running);
    assert_eq!(status.created_at_ns, 1_000);
    assert_eq!(status.started_at_ns, Some(1_010));
    assert_eq!(status.completed_at_ns, None);
    assert_eq!(status.result, None);
    assert_eq!(status.error, None);
    assert_eq!(status.merged_into, None);
}

#[test]
fn operation_status_splits_completed_result_and_failed_error() {
    let ids = operation_ids(2);
    let mut operations = OperationStore::new();
    operations
        .request_operation(operation_request(
            ids[0],
            OperationType::Start,
            "svc",
            1_000,
        ))
        .expect("request");
    operations
        .complete_operation(ids[0], 1_020, "active")
        .expect("complete");
    operations
        .request_operation(operation_request(ids[1], OperationType::Stop, "svc", 1_100))
        .expect("request stop");
    operations
        .fail_operation(ids[1], 1_120, "timeout")
        .expect("fail stop");

    let completed = operation_status(&operations, ids[0]).expect("completed");
    assert_eq!(completed.state, OperationState::Completed);
    assert_eq!(completed.result.as_deref(), Some("active"));
    assert_eq!(completed.error, None);

    let failed = operation_status(&operations, ids[1]).expect("failed");
    assert_eq!(failed.state, OperationState::Failed);
    assert_eq!(failed.result, None);
    assert_eq!(failed.error.as_deref(), Some("timeout"));
}

#[test]
fn operation_status_reports_merged_target() {
    let ids = operation_ids(2);
    let mut operations = OperationStore::new();
    operations
        .request_operation(operation_request(
            ids[0],
            OperationType::Start,
            "svc",
            1_000,
        ))
        .expect("first start");
    let outcome = operations
        .request_operation(operation_request(
            ids[1],
            OperationType::Start,
            "svc",
            1_010,
        ))
        .expect("merged start");
    assert_eq!(
        outcome.decision,
        OperationConflictDecision::MergeIntoExisting {
            existing_id: ids[0],
        }
    );

    let merged = operation_status(&operations, ids[1]).expect("merged");

    assert_eq!(merged.state, OperationState::Merged);
    assert_eq!(merged.merged_into, Some(ids[0]));
    assert_eq!(merged.result, None);
    assert_eq!(merged.error, None);
}

#[test]
fn operation_status_reports_unknown_operation() {
    let ids = operation_ids(1);
    let operations = OperationStore::new();

    let err = operation_status(&operations, ids[0]).expect_err("unknown");

    assert_eq!(
        err,
        QueryError::UnknownOperation {
            operation_id: ids[0],
        }
    );
}

#[test]
fn operation_status_reports_unknown_after_terminal_retention_expiry() {
    let ids = operation_ids(1);
    let mut operations = OperationStore::new();
    operations
        .request_operation(operation_request(
            ids[0],
            OperationType::Start,
            "svc",
            1_000,
        ))
        .expect("request");
    operations
        .complete_operation(ids[0], 1_010, "active")
        .expect("complete");
    assert_eq!(
        operations.purge_terminal_retained_until(
            1_010 + DEFAULT_TERMINAL_OPERATION_RETENTION_NS,
            DEFAULT_TERMINAL_OPERATION_RETENTION_NS,
        ),
        vec![ids[0]]
    );

    let err = operation_status(&operations, ids[0]).expect_err("unknown after purge");

    assert_eq!(
        err,
        QueryError::UnknownOperation {
            operation_id: ids[0],
        }
    );
}
