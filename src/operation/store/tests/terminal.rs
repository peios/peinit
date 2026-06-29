use crate::ids::OperationId;
use crate::operation::store::{
    DEFAULT_TERMINAL_OPERATION_RETENTION_NS, OperationEventDetail, OperationStore,
    OperationStoreError,
};
use crate::operation::{
    OperationState, OperationTransitionAction, OperationTransitionError, OperationType,
};

use super::{ids, request};

#[test]
fn completion_removes_active_operation_and_terminal_purge_drops_retained_record() {
    let ids = ids(1);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("start");
    store.start_operation(ids[0], 1_005).expect("running");

    let event = store
        .complete_operation(ids[0], 1_040, "active")
        .expect("complete");

    assert_eq!(store.active_for_service("svc"), Vec::<OperationId>::new());
    assert_eq!(
        event.detail,
        OperationEventDetail::Completed {
            duration_ns: 40,
            result: "active".to_string(),
        }
    );
    assert_eq!(store.purge_terminal_completed_at_or_before(1_039), vec![]);
    assert_eq!(
        store.get(ids[0]).expect("retained").state,
        OperationState::Completed
    );
    assert_eq!(
        store.purge_terminal_completed_at_or_before(1_040),
        vec![ids[0]]
    );
    assert!(store.get(ids[0]).is_none());
}

#[test]
fn terminal_retention_deadline_and_purge_use_default_grace_window() {
    let ids = ids(1);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("start");
    store
        .complete_operation(ids[0], 1_040, "active")
        .expect("complete");

    assert_eq!(
        store.next_terminal_retention_deadline_ns(DEFAULT_TERMINAL_OPERATION_RETENTION_NS),
        Some(1_040 + DEFAULT_TERMINAL_OPERATION_RETENTION_NS)
    );
    assert_eq!(
        store.purge_terminal_retained_until(
            1_039 + DEFAULT_TERMINAL_OPERATION_RETENTION_NS,
            DEFAULT_TERMINAL_OPERATION_RETENTION_NS,
        ),
        vec![]
    );
    assert_eq!(
        store.purge_terminal_retained_until(
            1_040 + DEFAULT_TERMINAL_OPERATION_RETENTION_NS,
            DEFAULT_TERMINAL_OPERATION_RETENTION_NS,
        ),
        vec![ids[0]]
    );
    assert!(store.get(ids[0]).is_none());
}

#[test]
fn duplicate_operation_id_is_rejected_without_mutating_store() {
    let ids = ids(1);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("start");

    let err = store
        .request_operation(request(ids[0], OperationType::Stop, 1_010))
        .expect_err("duplicate id");

    assert_eq!(
        err,
        OperationStoreError::DuplicateOperationId { id: ids[0] }
    );
    assert_eq!(store.active_for_service("svc"), vec![ids[0]]);
    assert_eq!(
        store.get(ids[0]).expect("record").operation_type,
        OperationType::Start
    );
}

#[test]
fn invalid_transition_from_store_reports_transition_error() {
    let ids = ids(1);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("start");
    store
        .complete_operation(ids[0], 1_010, "active")
        .expect("complete");

    let err = store
        .start_operation(ids[0], 1_020)
        .expect_err("cannot start completed operation");

    assert_eq!(
        err,
        OperationStoreError::Transition(OperationTransitionError::InvalidTransition {
            id: ids[0],
            from: OperationState::Completed,
            action: OperationTransitionAction::Start,
        })
    );
}
