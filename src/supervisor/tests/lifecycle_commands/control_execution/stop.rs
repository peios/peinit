use crate::boundary::ProcessSignal;
use crate::execution::control::ControlOperationKind;
use crate::operation::OperationState;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::tests::TestProcessController;

use super::*;

#[test]
fn stop_execution_sends_sigterm_and_completes_when_process_exits() {
    let mut supervisor = active_app_supervisor();
    let operation_id = stop_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    let dispatch = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop dispatch");

    assert_eq!(dispatch.execution.kind, ControlOperationKind::Stop);
    assert_eq!(controller.signals.len(), 1);
    assert_eq!(controller.signals[0].signal, ProcessSignal::Sigterm);
    assert_eq!(controller.signals[0].target.pid, 8000);
    assert_eq!(controller.signals[0].target.pidfd, 50);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Stopping,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("stop operation")
            .state,
        OperationState::Running,
    );
    assert_eq!(
        supervisor
            .next_stop_timeout_deadline()
            .expect("stop timeout")
            .due_at_ns,
        LIFECYCLE_COMMAND_NS + 10_000_000_000,
    );

    let job = current_app_job(&supervisor);
    let terminal = supervisor
        .complete_job(job, EXIT_NS, 0)
        .expect("complete stopped app");

    assert!(terminal.restart_start_dispatches.is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("stop operation")
            .state,
        OperationState::Completed,
    );
    assert!(supervisor.next_stop_timeout_deadline().is_none());
}

#[test]
fn due_stop_timeout_kills_service_cgroup_once() {
    let mut supervisor = active_app_supervisor();
    stop_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop dispatch");
    let due_at_ns = supervisor
        .next_stop_timeout_deadline()
        .expect("deadline")
        .due_at_ns;

    let escalation = supervisor
        .process_next_due_stop_timeout(&mut controller, due_at_ns)
        .expect("process timeout")
        .expect("escalation");

    assert_eq!(escalation.escalation.service, "app");
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/main".to_string()],
    );
    assert!(supervisor.next_stop_timeout_deadline().is_none());
}

#[test]
fn post_kill_stop_cleanup_empty_cgroup_finishes_stop_and_removes_tree() {
    let mut supervisor = active_app_supervisor();
    let operation_id = stop_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop dispatch");
    let due_at_ns = supervisor
        .next_stop_timeout_deadline()
        .expect("deadline")
        .due_at_ns;
    supervisor
        .process_next_due_stop_timeout(&mut controller, due_at_ns)
        .expect("process timeout")
        .expect("escalation");
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/app/main", false);

    let cleanup_due = due_at_ns + 5_000_000_000;
    assert!(
        supervisor
            .process_due_cgroup_cleanups(&mut controller, cleanup_due)
            .expect("cleanup")
    );

    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Inactive);
    assert_eq!(status.cause, Some(TransitionCause::ExplicitStop));
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("stop operation")
            .state,
        OperationState::Completed,
    );
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/app/main"]
    );
    assert_eq!(
        controller.cgroup_removes,
        vec![
            "/sys/fs/cgroup/peinit/app/main",
            "/sys/fs/cgroup/peinit/app/hooks",
            "/sys/fs/cgroup/peinit/app/health",
            "/sys/fs/cgroup/peinit/app",
        ]
    );
}

#[test]
fn post_kill_stop_cleanup_populated_cgroup_marks_abandoned_and_fails_stop() {
    let mut supervisor = active_app_supervisor();
    let operation_id = stop_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop dispatch");
    let due_at_ns = supervisor
        .next_stop_timeout_deadline()
        .expect("deadline")
        .due_at_ns;
    supervisor
        .process_next_due_stop_timeout(&mut controller, due_at_ns)
        .expect("process timeout")
        .expect("escalation");

    let cleanup_due = due_at_ns + 5_000_000_000;
    assert!(
        supervisor
            .process_due_cgroup_cleanups(&mut controller, cleanup_due)
            .expect("cleanup")
    );

    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Abandoned);
    assert_eq!(status.cause, Some(TransitionCause::ProcessUnkillable));
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("stop operation")
            .state,
        OperationState::Failed,
    );
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/app/main"]
    );
    assert!(controller.cgroup_removes.is_empty());
}
