use crate::control::lifecycle::{
    LifecycleCommand, LifecycleCommandError, LifecycleCommandOutcome, admit_lifecycle_command,
};
use crate::operation::OperationType;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::OperationStore;
use crate::service::runtime::ServiceState;

use super::super::{command_request, ids, operation_request, service_table, transition_to};

/// §8.3, `Restart` + `Start`: merge, a restart already includes a start. The
/// matrix reads the service as Stopping and expected a queue, which the
/// conflict table's merge is not (PEI-824).
#[test]
fn start_while_a_restart_stop_leg_runs_merges_into_the_restart() {
    let ids = ids(2);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Stopping);
    let mut operations = OperationStore::new();
    operations
        .request_operation(operation_request(
            ids[0],
            OperationType::Restart,
            "svc",
            1_000,
        ))
        .expect("restart requested");
    operations
        .start_operation(ids[0], 1_001)
        .expect("restart running");

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[1], LifecycleCommand::Start, "svc", 1_002),
    )
    .expect("start admitted");

    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected operation outcome");
    };
    assert_eq!(operation.returned_operation_id, ids[0]);
    assert_eq!(
        operation.decision,
        OperationConflictDecision::MergeIntoExisting {
            existing_id: ids[0]
        }
    );
    assert_eq!(operations.active_for_service("svc"), vec![ids[0]]);
}

#[test]
fn start_on_inactive_service_creates_start_operation() {
    let ids = ids(1);
    let mut services = service_table();
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Start, "svc", 1_000),
    )
    .expect("start admitted");

    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected operation outcome");
    };
    assert_eq!(operation.returned_operation_id, ids[0]);
    assert_eq!(operation.decision, OperationConflictDecision::CreateNew);
    assert_eq!(operations.active_for_service("svc"), vec![ids[0]]);
}

#[test]
fn start_on_active_service_returns_already_without_operation() {
    let ids = ids(1);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Active);
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Start, "svc", 1_000),
    )
    .expect("already active");

    assert!(matches!(outcome, LifecycleCommandOutcome::Already(_)));
    assert!(operations.active_for_service("svc").is_empty());
}

#[test]
fn start_on_starting_service_must_merge_into_existing_start() {
    let ids = ids(2);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Starting);
    let mut operations = OperationStore::new();
    operations
        .request_operation(operation_request(
            ids[0],
            OperationType::Start,
            "svc",
            1_000,
        ))
        .expect("existing start");

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[1], LifecycleCommand::Start, "svc", 1_010),
    )
    .expect("merge");

    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected operation outcome");
    };
    assert_eq!(
        operation.decision,
        OperationConflictDecision::MergeIntoExisting {
            existing_id: ids[0],
        }
    );
    assert_eq!(operation.returned_operation_id, ids[0]);
    assert_eq!(operations.active_for_service("svc"), vec![ids[0]]);
}

#[test]
fn expected_merge_without_active_operation_is_rejected_without_mutation() {
    let ids = ids(1);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Starting);
    let mut operations = OperationStore::new();

    let err = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Start, "svc", 1_000),
    )
    .expect_err("missing merge target");

    assert_eq!(
        err,
        LifecycleCommandError::ExpectedMerge {
            service: "svc".to_string(),
            command: LifecycleCommand::Start,
        }
    );
    assert!(operations.get(ids[0]).is_none());
}

#[test]
fn start_on_backoff_creates_deferred_start_without_planning_immediate_launch() {
    let ids = ids(1);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Backoff);
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Start, "svc", 1_000),
    )
    .expect("deferred start");

    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected operation outcome");
    };
    assert_eq!(operation.returned_operation_id, ids[0]);
    assert_eq!(operation.decision, OperationConflictDecision::CreateNew);
    assert_eq!(operations.active_for_service("svc"), vec![ids[0]]);
}

#[test]
fn start_on_backoff_merges_with_existing_deferred_start() {
    let ids = ids(2);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Backoff);
    let mut operations = OperationStore::new();
    admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Start, "svc", 1_000),
    )
    .expect("deferred start");

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[1], LifecycleCommand::Start, "svc", 1_010),
    )
    .expect("merge deferred start");

    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected operation outcome");
    };
    assert_eq!(
        operation.decision,
        OperationConflictDecision::MergeIntoExisting {
            existing_id: ids[0],
        }
    );
    assert_eq!(operation.returned_operation_id, ids[0]);
    assert_eq!(operations.active_for_service("svc"), vec![ids[0]]);
}
