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

// PEI-359. The warning §5.3 asks for existed only as an *operation result*,
// returned to a `wait=true` caller — and `reload` defaults to `wait=false`, so
// the default way to issue one produced no record at all when the service
// announced a reload and never finished it.
//
// That is the case the whole detection protocol exists to catch. A service
// that says RELOADING=1 and then never says READY=1 has wedged mid-reload or
// lost its handler, and it is strictly worse than one that never implements
// the handshake — that one at least resolves as advisory for a known reason.
#[test]
fn an_unconfirmed_reload_is_audited_and_reported_rather_than_only_returned() {
    let mut supervisor = active_app_supervisor();
    reload_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload dispatch");
    apply_notify(
        &mut supervisor,
        datagram(8000, b"RELOADING=1"),
        RELOAD_NOTIFY_NS,
    )
    .expect("apply reloading notification");
    let deadline = supervisor
        .next_reload_detection_deadline()
        .expect("reload extended wait");

    let completions = supervisor
        .process_due_reload_detection_windows(deadline.due_at_ns)
        .expect("complete extended wait");

    // The phase reaches the dispatch, which is what the console and the audit
    // stream key on. Without it the two outcomes are indistinguishable there.
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].completion.phase, ReloadDetectionPhase::ExtendedWait);
}

// The ordinary outcome is not a diagnostic. A service that never implements
// the handshake lets its detection window expire on every reload, and that
// must stay quiet or the signal is worthless.
#[test]
fn an_expired_detection_window_is_not_reported_as_a_failed_reload() {
    let mut supervisor = active_app_supervisor();
    reload_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload dispatch");

    let completions = supervisor
        .process_due_reload_detection_windows(CONTROL_NS + 2_000_000_000)
        .expect("complete detection window");

    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].completion.phase,
        ReloadDetectionPhase::DetectionWindow,
    );
}
