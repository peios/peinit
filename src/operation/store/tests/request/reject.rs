use crate::operation::OperationType;
use crate::operation::conflict::OperationConflictRejection;
use crate::operation::store::{OperationStore, OperationStoreError};

use super::super::{ids, request};

#[test]
fn reset_is_rejected_while_operation_is_active() {
    let ids = ids(2);
    let mut store = OperationStore::new();
    store
        .request_operation(request(ids[0], OperationType::Start, 1_000))
        .expect("start");

    let err = store
        .request_operation(request(ids[1], OperationType::Reset, 1_010))
        .expect_err("reset rejected");

    assert_eq!(
        err,
        OperationStoreError::ConflictRejected(
            OperationConflictRejection::ResetWhileOperationInProgress
        )
    );
    assert_eq!(store.active_for_service("svc"), vec![ids[0]]);
    assert!(store.get(ids[1]).is_none());
}
