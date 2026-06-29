use crate::boot::BootMode;
use crate::boot::phase2::prepare_phase2_boot_plan;
use crate::ids::OperationIdAllocator;
use crate::service::ServiceDefinition;

use super::{
    OperationRecord, OperationSource, OperationState, OperationTransitionAction,
    OperationTransitionError, OperationType, TokenSummary, boot_start_operations_from_phase2_plan,
};

fn operation_id() -> crate::ids::OperationId {
    OperationIdAllocator::new()
        .allocate_batch(1, 1_717_171_717_123_456_789)
        .expect("operation id")[0]
}

fn operation() -> OperationRecord {
    OperationRecord::new(
        operation_id(),
        OperationType::Start,
        "app",
        OperationSource::Admin,
        Some(TokenSummary::requested_identity("SYSTEM")),
        1_000,
    )
}

#[test]
fn new_operation_starts_pending() {
    let operation = operation();

    assert_eq!(operation.state, OperationState::Pending);
    assert_eq!(operation.operation_type, OperationType::Start);
    assert_eq!(operation.service, "app");
    assert_eq!(operation.source, OperationSource::Admin);
    assert!(!operation.state.is_terminal());
    assert_eq!(
        operation.caller,
        Some(TokenSummary::requested_identity("SYSTEM"))
    );
}

#[test]
fn pending_operation_can_start_and_complete() {
    let mut operation = operation();

    operation.start(1_010).expect("start");
    operation.complete(1_050, "active").expect("complete");

    assert_eq!(operation.state, OperationState::Completed);
    assert!(operation.state.is_terminal());
    assert_eq!(operation.started_at_ns, Some(1_010));
    assert_eq!(operation.completed_at_ns, Some(1_050));
    assert_eq!(operation.duration_ns(), Some(50));
    assert_eq!(operation.result.as_deref(), Some("active"));
}

#[test]
fn pending_operation_can_fail_before_execution() {
    let mut operation = operation();

    operation.fail(1_020, "validation_error").expect("fail");

    assert_eq!(operation.state, OperationState::Failed);
    assert_eq!(operation.started_at_ns, None);
    assert_eq!(operation.result.as_deref(), Some("validation_error"));
}

#[test]
fn merged_operation_records_target_operation() {
    let mut operation = operation();
    let target = OperationIdAllocator::new()
        .allocate_batch(2, 1_717_171_717_123_456_789)
        .expect("operation ids")[1];

    operation.merge_into(target, 1_015).expect("merge");

    assert_eq!(operation.state, OperationState::Merged);
    assert_eq!(operation.completed_at_ns, Some(1_015));
    assert_eq!(operation.merged_into, Some(target));
}

#[test]
fn running_operation_can_abort() {
    let mut operation = operation();

    operation.start(1_010).expect("start");
    operation.abort(1_040, "superseded").expect("abort");

    assert_eq!(operation.state, OperationState::Aborted);
    assert_eq!(operation.result.as_deref(), Some("superseded"));
    assert_eq!(operation.duration_ns(), Some(40));
}

#[test]
fn invalid_transition_does_not_change_operation() {
    let mut operation = operation();
    let before = operation.clone();

    let err = operation
        .abort(1_040, "cannot abort pending")
        .expect_err("invalid");

    assert_eq!(operation, before);
    assert_eq!(
        err,
        OperationTransitionError::InvalidTransition {
            id: before.id,
            from: OperationState::Pending,
            action: OperationTransitionAction::Abort,
        }
    );
}

#[test]
fn transition_timestamps_cannot_precede_creation() {
    let mut operation = operation();

    let err = operation.start(999).expect_err("start before creation");

    assert_eq!(
        err,
        OperationTransitionError::StartBeforeCreation {
            id: operation.id,
            created_at_ns: 1_000,
            started_at_ns: 999,
        }
    );
    assert_eq!(operation.state, OperationState::Pending);
}

#[test]
fn phase2_plan_creates_boot_start_operations_in_start_order() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("authd".to_string());
    let mut authd = ServiceDefinition::simple_system_boot("authd", "/sbin/authd");
    authd.triggers.clear();
    let mut operations = OperationIdAllocator::new();
    let mut jobs = crate::ids::JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        &[app, authd],
        10,
        1_717_171_717_123_456_789,
        &mut operations,
        &mut jobs,
    )
    .expect("phase2 plan");

    let records = boot_start_operations_from_phase2_plan(&plan);

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].service, "authd");
    assert_eq!(records[1].service, "app");
    assert_eq!(records[0].id, plan.starts[0].operation_id);
    assert_eq!(records[0].operation_type, OperationType::Start);
    assert_eq!(records[0].source, OperationSource::Boot);
    assert_eq!(records[0].state, OperationState::Pending);
    assert_eq!(records[0].created_at_ns, plan.observed_at_ns);
}
