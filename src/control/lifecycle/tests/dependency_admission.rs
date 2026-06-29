use crate::control::lifecycle::{
    LifecycleCommand, LifecycleCommandError, LifecycleCommandOutcome,
    admit_lifecycle_command_with_operation_ids,
};
use crate::ids::{OperationId, OperationIdAllocator};
use crate::operation::store::OperationStore;
use crate::operation::{OperationSource, OperationState};
use crate::service::{ServiceDefinition, ServiceTable};

use super::{OBSERVED_AT_NS, command_request};

fn service(name: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"))
}

fn table(definitions: Vec<ServiceDefinition>) -> ServiceTable {
    ServiceTable::from_boot_snapshot(definitions).expect("service table")
}

fn request_id(operation_ids: &mut OperationIdAllocator) -> OperationId {
    operation_ids
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("request id")[0]
}

fn expected_operation_ids(count: usize) -> Vec<OperationId> {
    OperationIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("operation ids")
}

#[test]
fn start_admission_dispatches_dependency_operations_before_requested_start() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut services = table(vec![app, service("db")]);
    let expected_ids = expected_operation_ids(2);
    let mut operation_ids = OperationIdAllocator::new();
    let requested_id = request_id(&mut operation_ids);
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command_with_operation_ids(
        &mut services,
        &mut operations,
        &mut operation_ids,
        command_request(requested_id, LifecycleCommand::Start, "app", OBSERVED_AT_NS),
    )
    .expect("start admitted");

    let LifecycleCommandOutcome::OnDemandStart(dispatch) = outcome else {
        panic!("expected on-demand start outcome");
    };
    assert_eq!(
        dispatch.requested_operation.returned_operation_id,
        requested_id
    );
    assert_eq!(dispatch.dependency_operations.len(), 1);
    assert_eq!(
        dispatch.dependency_operations[0].returned_operation_id,
        expected_ids[1]
    );
    assert_eq!(
        dispatch
            .events
            .iter()
            .map(|event| event.service.as_str())
            .collect::<Vec<_>>(),
        vec!["db", "app"]
    );
    assert_eq!(operations.active_for_service("db"), vec![expected_ids[1]]);
    assert_eq!(operations.active_for_service("app"), vec![requested_id]);
    assert_eq!(
        operations.get(expected_ids[1]).expect("dependency").source,
        OperationSource::DependencyPropagation,
    );
    assert_eq!(
        operations.get(requested_id).expect("requested").source,
        OperationSource::Admin,
    );
    assert_eq!(operation_ids.next_sequence(), 2);
}

#[test]
fn blocked_start_admission_fails_requested_operation_without_allocating_dependencies() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut services = table(vec![app]);
    let mut operation_ids = OperationIdAllocator::new();
    let requested_id = request_id(&mut operation_ids);
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command_with_operation_ids(
        &mut services,
        &mut operations,
        &mut operation_ids,
        command_request(requested_id, LifecycleCommand::Start, "app", OBSERVED_AT_NS),
    )
    .expect("blocked start admitted");

    let LifecycleCommandOutcome::OnDemandStart(dispatch) = outcome else {
        panic!("expected on-demand start outcome");
    };
    assert!(dispatch.dependency_operations.is_empty());
    assert_eq!(
        operations.get(requested_id).expect("requested").state,
        OperationState::Failed,
    );
    assert!(operations.active_for_service("app").is_empty());
    assert_eq!(operation_ids.next_sequence(), 1);
}

#[test]
fn cycle_in_start_plan_does_not_mutate_operations_or_allocator() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut db = service("db");
    db.requires.push("app".to_string());
    let mut services = table(vec![app, db]);
    let mut operation_ids = OperationIdAllocator::new();
    let requested_id = request_id(&mut operation_ids);
    let mut operations = OperationStore::new();

    let error = admit_lifecycle_command_with_operation_ids(
        &mut services,
        &mut operations,
        &mut operation_ids,
        command_request(requested_id, LifecycleCommand::Start, "app", OBSERVED_AT_NS),
    )
    .expect_err("cycle error");

    assert!(matches!(error, LifecycleCommandError::StartPlan(_)));
    assert_eq!(operation_ids.next_sequence(), 1);
    assert!(operations.active_for_service("app").is_empty());
    assert!(operations.get(requested_id).is_none());
}

#[test]
fn dependency_id_exhaustion_does_not_mutate_operations_or_allocator() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut services = table(vec![app, service("db")]);
    let requested_id = expected_operation_ids(1)[0];
    let mut operation_ids = OperationIdAllocator::with_next_sequence(u64::MAX);
    let mut operations = OperationStore::new();

    let error = admit_lifecycle_command_with_operation_ids(
        &mut services,
        &mut operations,
        &mut operation_ids,
        command_request(requested_id, LifecycleCommand::Start, "app", OBSERVED_AT_NS),
    )
    .expect_err("allocation error");

    assert!(matches!(error, LifecycleCommandError::StartDispatch(_)));
    assert_eq!(operation_ids.next_sequence(), u64::MAX);
    assert!(operations.active_for_service("app").is_empty());
    assert!(operations.active_for_service("db").is_empty());
    assert!(operations.get(requested_id).is_none());
}
