use crate::control::lifecycle::{
    LifecycleCommand, LifecycleCommandError, LifecycleCommandOutcome, admit_lifecycle_command,
};
use crate::operation::OperationState;
use crate::operation::store::OperationStore;
use crate::service::runtime::ServiceState;

use super::{command_request, ids, service_table, transition_to};

#[test]
fn stop_on_completed_service_clears_to_inactive_synchronously() {
    let ids = ids(1);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Completed);
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Stop, "svc", 1_000),
    )
    .expect("stop clear");

    let LifecycleCommandOutcome::SynchronousClear(clear) = outcome else {
        panic!("expected synchronous clear");
    };
    assert_eq!(clear.request.returned_operation_id, ids[0]);
    assert_eq!(clear.service_transition.event.to, ServiceState::Inactive);
    assert_eq!(clear.completed.operation_id, ids[0]);
    assert_eq!(
        operations.get(ids[0]).expect("retained operation").state,
        OperationState::Completed,
    );
    assert!(operations.active_for_service("svc").is_empty());
    assert_eq!(
        services.runtime("svc").expect("runtime").state,
        ServiceState::Inactive,
    );
}

#[test]
fn reset_on_failed_service_clears_to_inactive_synchronously() {
    let ids = ids(1);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Failed);
    let mut operations = OperationStore::new();

    let outcome = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Reset, "svc", 1_000),
    )
    .expect("reset");

    assert!(matches!(
        outcome,
        LifecycleCommandOutcome::SynchronousClear(_)
    ));
    assert_eq!(
        services.runtime("svc").expect("runtime").state,
        ServiceState::Inactive,
    );
}

#[test]
fn start_restart_and_reload_reject_definition_removed_entries_but_stop_can_drain() {
    let ids = ids(2);
    let mut services = service_table();
    transition_to(&mut services, ServiceState::Active);
    services
        .apply_definition_snapshot(Vec::new())
        .expect("definition removed");
    let mut operations = OperationStore::new();

    let err = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[0], LifecycleCommand::Start, "svc", 1_000),
    )
    .expect_err("definition removed start");
    assert_eq!(
        err,
        LifecycleCommandError::DefinitionRemoved {
            service: "svc".to_string(),
        }
    );

    let stop = admit_lifecycle_command(
        &mut services,
        &mut operations,
        command_request(ids[1], LifecycleCommand::Stop, "svc", 1_010),
    )
    .expect("stop retained removed service");
    assert!(matches!(
        stop,
        LifecycleCommandOutcome::OperationAccepted(_)
    ));
    assert_eq!(operations.active_for_service("svc"), vec![ids[1]]);
}
