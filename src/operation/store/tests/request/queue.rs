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
