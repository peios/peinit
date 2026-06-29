use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::{OperationEventDetail, OperationStore};
use crate::operation::{OperationState, OperationType};

use super::super::{event_details, ids, request};

#[test]
fn request_without_conflict_creates_pending_operation() {
    let ids = ids(1);
    let mut store = OperationStore::new();

    let outcome = store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("request operation");

    assert_eq!(outcome.returned_operation_id, ids[0]);
    assert_eq!(outcome.stored_operation_id, ids[0]);
    assert_eq!(outcome.decision, OperationConflictDecision::CreateNew);
    assert_eq!(store.active_for_service("svc"), vec![ids[0]]);
    assert_eq!(
        store.get(ids[0]).expect("record").state,
        OperationState::Pending
    );
    assert_eq!(
        event_details(&outcome.events),
        vec![OperationEventDetail::Requested]
    );
}

#[test]
fn same_type_request_records_merged_operation_and_returns_existing_id() {
    let ids = ids(2);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("first start");

    let outcome = store
        .request_operation(request(ids[1], OperationType::Start, 1_010))
        .expect("merged start");

    assert_eq!(outcome.returned_operation_id, ids[0]);
    assert_eq!(outcome.stored_operation_id, ids[1]);
    assert_eq!(
        outcome.decision,
        OperationConflictDecision::MergeIntoExisting {
            existing_id: ids[0],
        }
    );
    assert_eq!(store.active_for_service("svc"), vec![ids[0]]);
    let merged = store.get(ids[1]).expect("merged record");
    assert_eq!(merged.state, OperationState::Merged);
    assert_eq!(merged.merged_into, Some(ids[0]));
    assert_eq!(
        event_details(&outcome.events),
        vec![
            OperationEventDetail::Requested,
            OperationEventDetail::Merged {
                merged_into: ids[0],
            },
        ]
    );
}
