use crate::ids::OperationIdAllocator;
use crate::operation::store::OperationStore;
use crate::operation::{OperationSource, OperationState};

use super::super::admit_due_restart_policy_start;
use super::{
    DUE_AT_NS, NOW_NS, operation_ids_from_start, put_active_service_in_backoff, service, table,
};

#[test]
fn due_backoff_admits_restart_policy_start_and_dependencies() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut services = table(vec![app, service("db")]);
    put_active_service_in_backoff(&mut services, "app", DUE_AT_NS);
    let mut operations = OperationStore::new();
    let mut operation_ids = OperationIdAllocator::new();
    let expected_ids = operation_ids_from_start(2);

    let dispatch = admit_due_restart_policy_start(
        &services,
        &mut operations,
        &mut operation_ids,
        "app",
        NOW_NS,
    )
    .expect("restart policy start");

    assert_eq!(
        dispatch.requested_operation.returned_operation_id,
        expected_ids[0]
    );
    assert_eq!(
        dispatch.dependency_operations[0].returned_operation_id,
        expected_ids[1]
    );
    assert_eq!(
        operations.get(expected_ids[0]).expect("requested").source,
        OperationSource::RestartPolicy,
    );
    assert_eq!(
        operations.get(expected_ids[1]).expect("dependency").source,
        OperationSource::DependencyPropagation,
    );
    assert_eq!(
        operations.get(expected_ids[0]).expect("requested").state,
        OperationState::Pending,
    );
    assert_eq!(
        dispatch
            .events
            .iter()
            .map(|event| event.service.as_str())
            .collect::<Vec<_>>(),
        vec!["db", "app"]
    );
    assert_eq!(operation_ids.next_sequence(), 2);
}
