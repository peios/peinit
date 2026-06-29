use crate::control::lifecycle::{
    LifecycleCommand, LifecycleCommandOutcome, admit_lifecycle_command,
};
use crate::operation::OperationType;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::OperationStore;
use crate::service::runtime::ServiceState;

use super::super::{command_request, ids, operation_request, service_table, transition_to};

#[test]
fn restart_on_starting_service_queues_behind_running_start() {
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
    operations
        .start_operation(ids[0], 1_005)
        .expect("running start");

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[1], LifecycleCommand::Restart, "svc", 1_010),
    )
    .expect("restart queued");

    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected operation outcome");
    };
    assert_eq!(operation.decision, OperationConflictDecision::QueueNew);
    assert_eq!(operations.active_for_service("svc"), vec![ids[0], ids[1]]);
}
