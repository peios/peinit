use crate::control::lifecycle::{
    LifecycleCommand, LifecycleCommandError, LifecycleCommandOutcome,
    admit_lifecycle_command_with_operation_ids,
};
use crate::ids::{OperationId, OperationIdAllocator};
use crate::operation::store::OperationStore;
use crate::operation::{OperationSource, OperationState};
use crate::service::{ServiceDefinition, ServiceTable};

use super::{OBSERVED_AT_NS, command_request, service_table, transition_to};

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

/// §8.1: "If the service has no running process (Inactive, Completed, Failed,
/// Skipped), the stop phase is skipped and peinit proceeds directly to the
/// start phase. The operation type remains Restart for observability."
///
/// Only `Start` was routed to the on-demand start plan, so a `Restart` from one
/// of those four states reached the control boundary, where `process_target`
/// unconditionally looks for a live main job and errors with
/// `MissingCurrentMainJob` or `JobNotRunning`. Those are the states an operator
/// most often restarts from — a service that failed, a Oneshot that completed,
/// one somebody stopped earlier — so scripting a restart across a set of
/// services produced an error for every one that happened not to be running.
#[test]
fn restart_on_a_service_with_no_running_process_plans_a_start() {
    use crate::service::runtime::ServiceState;

    for state in [
        ServiceState::Inactive,
        ServiceState::Completed,
        ServiceState::Failed,
        ServiceState::Skipped,
    ] {
        let mut services = service_table();
        transition_to(&mut services, state);

        let mut operation_ids = OperationIdAllocator::new();
        let requested_id = request_id(&mut operation_ids);
        let mut operations = OperationStore::new();

        let outcome = admit_lifecycle_command_with_operation_ids(
            &mut services,
            &mut operations,
            &mut operation_ids,
            command_request(
                requested_id,
                LifecycleCommand::Restart,
                "svc",
                OBSERVED_AT_NS,
            ),
        )
        .unwrap_or_else(|error| panic!("restart from {state:?} was rejected: {error:?}"));

        assert!(
            matches!(outcome, LifecycleCommandOutcome::OnDemandStart(_)),
            "restart from {state:?} must plan a start, got {outcome:?}"
        );
    }
}

/// Backoff is deliberately *not* rerouted: the matrix gives it `Restart`, and a
/// service in Backoff has a restart pending, so the stop phase has something to
/// do. Without this the fix would quietly change a fifth state too.
#[test]
fn restart_in_backoff_still_takes_the_restart_path() {
    use crate::service::runtime::ServiceState;

    let mut services = service_table();
    transition_to(&mut services, ServiceState::Backoff);

    let mut operation_ids = OperationIdAllocator::new();
    let requested_id = request_id(&mut operation_ids);
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command_with_operation_ids(
        &mut services,
        &mut operations,
        &mut operation_ids,
        command_request(
            requested_id,
            LifecycleCommand::Restart,
            "svc",
            OBSERVED_AT_NS,
        ),
    )
    .expect("restart from Backoff admitted");

    assert!(
        matches!(outcome, LifecycleCommandOutcome::OperationAccepted(_)),
        "Backoff must keep the restart path, got {outcome:?}"
    );
}
