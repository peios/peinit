use crate::ids::OperationIdAllocator;
use crate::operation::store::{OperationRequest, OperationStore};
use crate::operation::{OperationSource, OperationType};

use super::super::{RestartPolicyAdmissionError, admit_due_restart_policy_start};
use super::{
    DUE_AT_NS, NOW_NS, operation_ids_from_start, put_active_service_in_backoff, service, table,
};

#[test]
fn dispatch_failure_rolls_back_allocated_restart_policy_id() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut services = table(vec![app, service("db")]);
    put_active_service_in_backoff(&mut services, "app", DUE_AT_NS);
    let expected_ids = operation_ids_from_start(2);
    let mut operations = OperationStore::new();
    operations
        .request_operation(OperationRequest {
            id: expected_ids[1],
            operation_type: OperationType::Start,
            service: "other".to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: NOW_NS,
        })
        .expect("preexisting duplicate dependency id");
    let original_operations = operations.clone();
    let mut operation_ids = OperationIdAllocator::new();

    let error = admit_due_restart_policy_start(
        &services,
        &mut operations,
        &mut operation_ids,
        "app",
        NOW_NS,
    )
    .expect_err("duplicate dependency id");

    assert!(matches!(
        error,
        RestartPolicyAdmissionError::StartDispatch(_)
    ));
    assert_eq!(operations, original_operations);
    assert_eq!(operation_ids.next_sequence(), 0);
}
