use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::{OperationEventDetail, OperationStore};
use crate::operation::{OperationState, OperationType};

use super::super::{event_details, ids, request};

#[test]
fn stop_cancels_pending_start_and_takes_active_slot() {
    let ids = ids(2);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("start");

    let outcome = store
        .request_operation(request(ids[1], OperationType::Stop, 1_010))
        .expect("stop");

    assert_eq!(
        outcome.decision,
        OperationConflictDecision::CancelExistingThenCreate {
            existing_id: ids[0],
        }
    );
    assert_eq!(store.active_for_service("svc"), vec![ids[1]]);
    assert_eq!(
        store.get(ids[0]).expect("cancelled").state,
        OperationState::Cancelled
    );
    assert_eq!(
        event_details(&outcome.events),
        vec![
            OperationEventDetail::Cancelled {
                reason: "superseded_by_later_operation".to_string(),
            },
            OperationEventDetail::Requested,
        ]
    );
}

#[test]
fn stop_aborts_running_start_and_takes_active_slot() {
    let ids = ids(2);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("start");
    store.start_operation(ids[0], 1_005).expect("start running");

    let outcome = store
        .request_operation(request(ids[1], OperationType::Stop, 1_020))
        .expect("stop");

    assert_eq!(
        outcome.decision,
        OperationConflictDecision::AbortExistingThenCreate {
            existing_id: ids[0],
        }
    );
    assert_eq!(store.active_for_service("svc"), vec![ids[1]]);
    assert_eq!(
        store.get(ids[0]).expect("aborted").state,
        OperationState::Aborted
    );
    assert_eq!(
        event_details(&outcome.events),
        vec![
            OperationEventDetail::Aborted {
                duration_ns: 20,
                reason: "superseded_by_later_operation".to_string(),
            },
            OperationEventDetail::Requested,
        ]
    );
}
