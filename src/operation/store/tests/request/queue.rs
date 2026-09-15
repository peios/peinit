use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::{OperationEventDetail, OperationStore};
use crate::operation::{OperationState, OperationType};

use super::super::{event_details, ids, request};

#[test]
fn start_queues_behind_stop() {
    let ids = ids(2);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Stop, 1_000))
        .expect("stop");

    let outcome = store
        .request_operation(request(ids[1], OperationType::Start, 1_010))
        .expect("start");

    assert_eq!(outcome.decision, OperationConflictDecision::QueueNew);
    assert_eq!(store.active_for_service("svc"), vec![ids[0], ids[1]]);
    assert_eq!(
        event_details(&outcome.events),
        vec![OperationEventDetail::Requested]
    );
}

/// A queued operation names what it waits for, and is ready only once that
/// has finished and it is the head of its service's queue (PEI-824).
#[test]
fn a_queued_restart_is_ready_once_the_stop_it_waits_for_finishes() {
    let ids = ids(2);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Stop, 1_000))
        .expect("stop");
    store.start_operation(ids[0], 1_001).expect("stop running");

    let outcome = store
        .request_operation(request(ids[1], OperationType::Restart, 1_010))
        .expect("restart");
    assert_eq!(outcome.decision, OperationConflictDecision::QueueNew);
    assert_eq!(store.queued_behind(ids[1]), Some(ids[0]));
    assert!(store.is_queued_behind_live(ids[1]));
    assert!(store.ready_queued_operations().is_empty());

    store
        .complete_operation(ids[0], 1_020, "inactive")
        .expect("stop completes");
    assert!(!store.is_queued_behind_live(ids[1]));
    let ready = store.ready_queued_operations();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, ids[1]);
    assert_eq!(ready[0].state, OperationState::Pending);

    store.clear_queued_behind(ids[1]);
    assert_eq!(store.queued_behind(ids[1]), None);
    assert!(store.ready_queued_operations().is_empty());
}

/// A queued operation that is itself terminated leaves the queue with the
/// rest of its bookkeeping.
#[test]
fn a_cancelled_queued_operation_is_forgotten() {
    let ids = ids(2);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Stop, 1_000))
        .expect("stop");
    store
        .request_operation(request(ids[1], OperationType::Restart, 1_010))
        .expect("restart");
    assert_eq!(store.queued_behind(ids[1]), Some(ids[0]));

    store
        .cancel_operation(ids[1], 1_020, "superseded")
        .expect("cancel restart");
    assert_eq!(store.queued_behind(ids[1]), None);
    assert_eq!(store.active_for_service("svc"), vec![ids[0]]);
}

#[test]
fn start_merges_with_queued_start_behind_stop() {
    let ids = ids(3);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Stop, 1_000))
        .expect("stop");
    store
        .request_operation(request(ids[1], OperationType::Start, 1_010))
        .expect("queued start");

    let outcome = store
        .request_operation(request(ids[2], OperationType::Start, 1_020))
        .expect("merged start");

    assert_eq!(
        outcome.decision,
        OperationConflictDecision::MergeIntoExisting {
            existing_id: ids[1],
        }
    );
    assert_eq!(outcome.returned_operation_id, ids[1]);
    assert_eq!(store.active_for_service("svc"), vec![ids[0], ids[1]]);
    assert_eq!(
        store.get(ids[2]).expect("merged").state,
        OperationState::Merged
    );
}
