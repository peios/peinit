use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::operation::{
    OPERATION_TIMEOUT_RESULT_CODE, OperationState, OperationType, is_operation_timeout_result,
};
use crate::service::ServiceDefinition;
use crate::supervisor::{PendingControlOperation, PendingControlRequirement};

use super::active_app_supervisor;
use crate::supervisor::tests::{LIFECYCLE_COMMAND_NS, ScriptedClock};

#[test]
fn restart_on_active_service_is_admitted_as_pending_operation_boundary() {
    let mut supervisor = active_app_supervisor();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);

    let dispatch = supervisor
        .restart_service("app", None, &mut clock)
        .expect("restart active app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = dispatch.outcome else {
        panic!("expected restart operation");
    };

    assert!(dispatch.context_id.is_none());
    assert!(dispatch.start_dispatches.is_empty());
    let operation_id = operation.returned_operation_id;
    let status = supervisor
        .operation_status(operation_id)
        .expect("restart operation");
    assert_eq!(status.operation_type, OperationType::Restart);
    assert_eq!(status.state, OperationState::Pending);
    assert_eq!(
        supervisor
            .service_status("app")
            .expect("app status")
            .current_operation
            .expect("current operation")
            .id,
        operation_id,
    );
    assert_pending_control(
        &dispatch.pending_control_operation,
        &supervisor,
        operation_id,
        OperationType::Restart,
        PendingControlRequirement::RestartProcess,
    );
}

#[test]
fn reload_on_active_service_is_queued_at_process_control_boundary() {
    let mut supervisor = active_app_supervisor();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);

    let dispatch = supervisor
        .reload_service("app", None, &mut clock)
        .expect("reload active app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = dispatch.outcome else {
        panic!("expected reload operation");
    };
    let operation_id = operation.returned_operation_id;

    assert!(dispatch.context_id.is_none());
    assert!(dispatch.start_dispatches.is_empty());
    assert_pending_control(
        &dispatch.pending_control_operation,
        &supervisor,
        operation_id,
        OperationType::Reload,
        PendingControlRequirement::ReloadProcess,
    );
}

#[test]
fn stop_supersedes_pending_restart_in_control_boundary_queue() {
    let mut supervisor = active_app_supervisor();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS, LIFECYCLE_COMMAND_NS + 1]);

    let restart = supervisor
        .restart_service("app", None, &mut clock)
        .expect("restart active app");
    let LifecycleCommandOutcome::OperationAccepted(restart_operation) = restart.outcome else {
        panic!("expected restart operation");
    };
    let restart_operation_id = restart_operation.returned_operation_id;

    let stop = supervisor
        .stop_service("app", None, &mut clock)
        .expect("stop active app");
    let LifecycleCommandOutcome::OperationAccepted(stop_operation) = stop.outcome else {
        panic!("expected stop operation");
    };
    let stop_operation_id = stop_operation.returned_operation_id;

    assert_eq!(
        supervisor
            .operation_status(restart_operation_id)
            .expect("cancelled restart")
            .state,
        OperationState::Cancelled,
    );
    assert_pending_control(
        &stop.pending_control_operation,
        &supervisor,
        stop_operation_id,
        OperationType::Stop,
        PendingControlRequirement::StopProcess,
    );
}

#[test]
fn pending_control_operation_times_out_from_creation_time() {
    let mut supervisor = active_app_supervisor();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);

    let stop = supervisor
        .stop_service("app", None, &mut clock)
        .expect("stop active app");
    let LifecycleCommandOutcome::OperationAccepted(stop_operation) = stop.outcome else {
        panic!("expected stop operation");
    };
    let operation_id = stop_operation.returned_operation_id;
    let timeout_ns =
        LIFECYCLE_COMMAND_NS + ServiceDefinition::DEFAULT_STOP_TIMEOUT_SECS * 1_000_000_000;

    let turn = supervisor
        .process_due_operation_maintenance(timeout_ns)
        .expect("operation maintenance");

    assert_eq!(turn.operation_timeouts.len(), 1);
    let status = supervisor
        .operation_status(operation_id)
        .expect("timed out operation");
    assert_eq!(status.state, OperationState::Failed);
    let error = status.error.as_deref().expect("timeout error");
    assert!(is_operation_timeout_result(error));
    assert!(error.starts_with(OPERATION_TIMEOUT_RESULT_CODE));
}

fn assert_pending_control(
    dispatch_pending: &Option<PendingControlOperation>,
    supervisor: &crate::supervisor::Supervisor,
    operation_id: crate::ids::OperationId,
    operation_type: OperationType,
    requirement: PendingControlRequirement,
) {
    let expected = PendingControlOperation {
        operation_id,
        service: "app".to_string(),
        operation_type,
        requirement,
    };
    assert_eq!(dispatch_pending, &Some(expected.clone()));
    assert_eq!(supervisor.pending_control_operations(), vec![expected],);
}
