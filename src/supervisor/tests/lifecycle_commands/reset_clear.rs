use crate::boundary::CgroupRemoveOutcome;
use crate::control::lifecycle::{LifecycleCommand, LifecycleCommandOutcome};
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;
use crate::supervisor::SupervisorError;

use super::{abandoned_task_supervisor, failed_notify_supervisor};
use crate::supervisor::tests::{LIFECYCLE_COMMAND_NS, ScriptedClock, TestProcessController};

#[test]
fn reset_on_failed_service_clears_state_and_completes_operation() {
    let mut supervisor = failed_notify_supervisor();
    assert_eq!(
        supervisor
            .service_status("task")
            .expect("task failed")
            .state,
        ServiceState::Failed,
    );
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS + 3]);

    let dispatch = supervisor
        .reset_service("task", None, &mut clock)
        .expect("reset failed task");
    let LifecycleCommandOutcome::SynchronousClear(clear) = dispatch.outcome else {
        panic!("expected synchronous clear");
    };

    let operation_id = clear.request.returned_operation_id;
    assert!(dispatch.context_id.is_none());
    assert!(dispatch.start_dispatches.is_empty());
    assert_eq!(clear.service_transition.event.to, ServiceState::Inactive);
    assert_eq!(
        supervisor
            .service_status("task")
            .expect("task status")
            .state,
        ServiceState::Inactive,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("clear operation")
            .state,
        OperationState::Completed,
    );
}

#[test]
fn direct_reset_on_abandoned_service_requires_process_controller() {
    let mut supervisor = abandoned_task_supervisor();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS + 3]);

    let error = supervisor
        .reset_service("task", None, &mut clock)
        .expect_err("abandoned reset without controller must fail");

    assert!(matches!(
        error,
        SupervisorError::Lifecycle(crate::control::lifecycle::LifecycleCommandError::InvalidState {
            service,
            command: LifecycleCommand::Reset,
            state: ServiceState::Abandoned,
        }) if service == "task"
    ));
    assert_eq!(
        supervisor
            .service_status("task")
            .expect("task status")
            .state,
        ServiceState::Abandoned,
    );
}

#[test]
fn reset_on_abandoned_empty_main_cgroup_cleans_tree_and_clears_state() {
    let mut supervisor = abandoned_task_supervisor();
    let mut controller = TestProcessController::default();
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/task/main", false);
    controller.set_cgroup_remove_result(
        "/sys/fs/cgroup/peinit/task/hooks",
        CgroupRemoveOutcome::Missing,
    );
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS + 3]);

    let dispatch = supervisor
        .run_lifecycle_command_with_process_controller(
            LifecycleCommand::Reset,
            "task",
            None,
            &mut controller,
            &mut clock,
        )
        .expect("reset abandoned task");
    let LifecycleCommandOutcome::SynchronousClear(clear) = dispatch.outcome else {
        panic!("expected synchronous clear");
    };

    assert_eq!(clear.service_transition.event.to, ServiceState::Inactive);
    assert!(dispatch.lifecycle_warnings.is_empty());
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/task/main"]
    );
    assert_eq!(
        controller.cgroup_removes,
        vec![
            "/sys/fs/cgroup/peinit/task/main",
            "/sys/fs/cgroup/peinit/task/hooks",
            "/sys/fs/cgroup/peinit/task/health",
            "/sys/fs/cgroup/peinit/task",
        ]
    );
    assert_eq!(
        supervisor
            .service_status("task")
            .expect("task status")
            .state,
        ServiceState::Inactive,
    );
}

#[test]
fn reset_on_abandoned_populated_main_cgroup_warns_and_leaves_cgroup() {
    let mut supervisor = abandoned_task_supervisor();
    let mut controller = TestProcessController::default();
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/task/main", true);
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS + 3]);

    let dispatch = supervisor
        .run_lifecycle_command_with_process_controller(
            LifecycleCommand::Reset,
            "task",
            None,
            &mut controller,
            &mut clock,
        )
        .expect("reset abandoned task");
    let LifecycleCommandOutcome::SynchronousClear(clear) = dispatch.outcome else {
        panic!("expected synchronous clear");
    };

    assert_eq!(clear.service_transition.event.to, ServiceState::Inactive);
    assert_eq!(
        dispatch.lifecycle_warnings,
        vec![
            "abandoned main cgroup for service task is still populated after reset -- cgroup remains leaked; underlying D-state process requires investigation"
        ]
    );
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/task/main"]
    );
    assert!(controller.cgroup_removes.is_empty());
    assert_eq!(
        supervisor
            .service_status("task")
            .expect("task status")
            .state,
        ServiceState::Inactive,
    );
}
