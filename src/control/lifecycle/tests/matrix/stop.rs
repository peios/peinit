use crate::control::lifecycle::{
    LifecycleCommand, LifecycleCommandOutcome, admit_lifecycle_command,
};
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::OperationStore;
use crate::operation::{OperationState, OperationType};
use crate::service::runtime::ServiceState;

use super::super::{command_request, ids, operation_request, service_table, transition_to};

/// §8.3, `Restart (Running)` + `Stop`: abort the restart, create the stop.
/// The matrix reads the service as Stopping and expected a merge, which the
/// conflict table's abort-and-create is not (PEI-824).
#[test]
fn stop_while_a_restart_stop_leg_runs_aborts_the_restart_and_creates_the_stop() {
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
        command_request(ids[1], LifecycleCommand::Stop, "svc", 1_002),
    )
    .expect("stop admitted");

    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected operation outcome");
    };
    assert_eq!(
        operation.decision,
        OperationConflictDecision::AbortExistingThenCreate {
            existing_id: ids[0]
        }
    );
    assert_eq!(
        operations.get(ids[0]).expect("restart").state,
        OperationState::Aborted
    );
    assert_eq!(operations.active_for_service("svc"), vec![ids[1]]);
}

#[test]
fn stop_on_inactive_service_is_noop() {
    let ids = ids(1);
    let mut services = service_table();
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Stop, "svc", 1_000),
    )
    .expect("noop");

    assert!(matches!(outcome, LifecycleCommandOutcome::Noop(_)));
    assert!(operations.active_for_service("svc").is_empty());
}
