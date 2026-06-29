use crate::ids::OperationIdAllocator;
use crate::operation::store::OperationStore;
use crate::service::runtime::ServiceState;

use super::super::{RestartPolicyAdmissionError, admit_due_restart_policy_start};
use super::{DUE_AT_NS, NOW_NS, put_active_service_in_backoff, service, table};

#[test]
fn backoff_not_due_does_not_mutate_operations_or_ids() {
    let mut services = table(vec![service("app")]);
    put_active_service_in_backoff(&mut services, "app", DUE_AT_NS);
    let mut operations = OperationStore::new();
    let mut operation_ids = OperationIdAllocator::new();

    let error = admit_due_restart_policy_start(
        &services,
        &mut operations,
        &mut operation_ids,
        "app",
        DUE_AT_NS - 1,
    )
    .expect_err("not due");

    assert_eq!(
        error,
        RestartPolicyAdmissionError::BackoffNotDue {
            service: "app".to_string(),
            due_at_ns: DUE_AT_NS,
            now_ns: DUE_AT_NS - 1,
        }
    );
    assert!(operations.active_for_service("app").is_empty());
    assert_eq!(operation_ids.next_sequence(), 0);
}

#[test]
fn non_backoff_service_is_rejected() {
    let services = table(vec![service("app")]);
    let mut operations = OperationStore::new();
    let mut operation_ids = OperationIdAllocator::new();

    let error = admit_due_restart_policy_start(
        &services,
        &mut operations,
        &mut operation_ids,
        "app",
        NOW_NS,
    )
    .expect_err("not in backoff");

    assert_eq!(
        error,
        RestartPolicyAdmissionError::NotInBackoff {
            service: "app".to_string(),
            state: ServiceState::Inactive,
        }
    );
}
