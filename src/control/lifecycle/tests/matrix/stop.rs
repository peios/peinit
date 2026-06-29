use crate::control::lifecycle::{
    LifecycleCommand, LifecycleCommandOutcome, admit_lifecycle_command,
};
use crate::operation::store::OperationStore;

use super::super::{command_request, ids, service_table};

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
