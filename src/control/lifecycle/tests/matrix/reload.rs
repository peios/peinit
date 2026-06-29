use crate::control::lifecycle::{
    LifecycleCommand, LifecycleCommandError, LifecycleCommandOutcome, admit_lifecycle_command,
};
use crate::operation::OperationType;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::OperationStore;
use crate::service::runtime::ServiceState;

use super::super::{command_request, ids, operation_request, service_table, transition_to};

#[test]
fn reload_on_inactive_service_is_invalid() {
    let ids = ids(1);
    let mut services = service_table();
    let mut operations = OperationStore::new();

    let err = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Reload, "svc", 1_000),
    )
    .expect_err("invalid reload");

    assert_eq!(
        err,
        LifecycleCommandError::InvalidState {
            service: "svc".to_string(),
            command: LifecycleCommand::Reload,
            state: ServiceState::Inactive,
        }
    );
}

#[test]
fn reload_on_reloading_service_merges() {
    let ids = ids(2);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Reloading);
    let mut operations = OperationStore::new();
    operations
        .request_operation(operation_request(
            ids[0],
            OperationType::Reload,
            "svc",
            1_000,
        ))
        .expect("existing reload");
    operations
        .start_operation(ids[0], 1_005)
        .expect("running reload");

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[1], LifecycleCommand::Reload, "svc", 1_010),
    )
    .expect("reload merge");

    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected operation outcome");
    };
    assert_eq!(
        operation.decision,
        OperationConflictDecision::MergeIntoExisting {
            existing_id: ids[0],
        }
    );
}

#[test]
fn stop_or_restart_supersedes_pending_reload_on_active_service() {
    for command in [LifecycleCommand::Stop, LifecycleCommand::Restart] {
        let ids = ids(2);
        let mut services = service_table();
        transition_to(&mut services, ServiceState::Active);
        let mut operations = OperationStore::new();
        operations
            .request_operation(operation_request(
                ids[0],
                OperationType::Reload,
                "svc",
                1_000,
            ))
            .expect("pending reload");

        let outcome = admit_lifecycle_command(
            &mut services,
            &mut operations,
            command_request(ids[1], command, "svc", 1_010),
        )
        .expect("pending reload superseded");

        let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
            panic!("expected operation outcome");
        };
        assert_eq!(
            operation.decision,
            OperationConflictDecision::CancelExistingThenCreate {
                existing_id: ids[0],
            },
        );
        assert_eq!(operation.returned_operation_id, ids[1]);
        assert_eq!(operations.active_for_service("svc"), vec![ids[1]]);
    }
}
