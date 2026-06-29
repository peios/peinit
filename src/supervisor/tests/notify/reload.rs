use crate::boundary::ProcessSignal;
use crate::execution::control::ReloadDetectionPhase;
use crate::execution::notify::NotifyAppliedField;
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;

use super::super::{ScriptedClock, TestProcessController};
use super::{
    CONTROL_NS, RELOAD_NOTIFY_NS, active_app_supervisor, apply_notify, datagram, reload_app,
};

#[test]
fn ready_notification_confirms_signal_reload() {
    let mut supervisor = active_app_supervisor();
    let operation_id = reload_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload dispatch");
    assert_eq!(controller.signals[0].signal, ProcessSignal::Sighup);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Reloading,
    );

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"RELOADING=1\nREADY=1"),
        RELOAD_NOTIFY_NS,
    )
    .expect("apply reload notification");

    assert_eq!(
        dispatch.notify.applied_fields,
        vec![NotifyAppliedField::Reloading, NotifyAppliedField::Ready],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    let operation = supervisor
        .operation_status(operation_id)
        .expect("reload operation");
    assert_eq!(operation.state, OperationState::Completed);
    assert_eq!(operation.result.as_deref(), Some("reload signal confirmed"));
    assert!(
        supervisor
            .due_reload_detection_deadlines(u64::MAX)
            .is_empty()
    );
}

#[test]
fn reloading_notification_extends_signal_reload_and_timeout_returns_advisory() {
    let mut supervisor = active_app_supervisor();
    let operation_id = reload_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload dispatch");

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"RELOADING=1"),
        RELOAD_NOTIFY_NS,
    )
    .expect("apply reloading notification");
    assert_eq!(
        dispatch.notify.applied_fields,
        vec![NotifyAppliedField::Reloading],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Reloading,
    );
    let deadline = supervisor
        .next_reload_detection_deadline()
        .expect("reload extended wait");
    assert_eq!(deadline.phase, ReloadDetectionPhase::ExtendedWait);

    let completions = supervisor
        .process_due_reload_detection_windows(deadline.due_at_ns)
        .expect("complete extended wait");

    assert_eq!(completions.len(), 1);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    let operation = supervisor
        .operation_status(operation_id)
        .expect("reload operation");
    assert_eq!(operation.state, OperationState::Completed);
    assert_eq!(
        operation.result.as_deref(),
        Some("reload signal advisory: service signalled RELOADING=1 but never completed reload"),
    );
}
