use crate::execution::notify::NotifyAppliedField;
use crate::service::runtime::ServiceState;
use crate::shutdown::{ShutdownKind, ShutdownStopDeadline};
use crate::supervisor::SupervisorError;

use super::super::{ScriptedClock, TestProcessController};
use super::{CONTROL_NS, active_app_supervisor, apply_notify, datagram};

const SHUTDOWN_NS: u64 = 9_000_000_000;
const EXTEND_NS: u64 = SHUTDOWN_NS + 1_000_000;

#[test]
fn shutdown_extend_timeout_resets_stop_deadline_and_clamps_to_service_cap() {
    let mut supervisor = active_app_supervisor();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=1000000000"),
        EXTEND_NS,
    )
    .expect("extend timeout");

    assert_eq!(
        dispatch.notify.applied_fields,
        vec![NotifyAppliedField::ExtendTimeoutUsec {
            value: "1000000000".to_string(),
        }],
    );
    let deadline = shutdown_deadline(&supervisor, "app");
    assert_eq!(deadline.started_at_ns, SHUTDOWN_NS);
    assert_eq!(deadline.due_at_ns, SHUTDOWN_NS + 40_000_000_000);
}

#[test]
fn shutdown_extend_timeout_updates_retained_stop_operation_deadline() {
    let mut supervisor = active_app_supervisor();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS - 1, CONTROL_NS]);
    supervisor
        .stop_service("app", None, &mut clock)
        .expect("request stop");
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop execution");
    let old_deadline = supervisor
        .next_stop_timeout_deadline()
        .expect("old stop deadline");
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=5000000"),
        EXTEND_NS,
    )
    .expect("extend timeout");

    let expected_due = EXTEND_NS + 5_000_000_000;
    let shutdown_deadline = shutdown_deadline(&supervisor, "app");
    assert_eq!(
        shutdown_deadline.operation_id,
        Some(old_deadline.operation_id)
    );
    assert_eq!(shutdown_deadline.due_at_ns, expected_due);
    assert_eq!(
        supervisor
            .next_stop_timeout_deadline()
            .expect("retained deadline")
            .due_at_ns,
        expected_due,
    );
}

#[test]
fn shutdown_extend_timeout_rejects_malformed_usec_value() {
    let mut supervisor = active_app_supervisor();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let error = apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=not-a-number"),
        EXTEND_NS,
    )
    .expect_err("invalid extension");

    assert!(matches!(
        error,
        SupervisorError::Shutdown(crate::shutdown::ShutdownError::InvalidTimeoutExtension {
            service,
            value,
        }) if service == "app" && value == "not-a-number"
    ));
}

#[test]
fn stopping_notify_prevents_later_stop_sigterm_but_keeps_stop_timeout() {
    let mut supervisor = active_app_supervisor();
    let mut controller = TestProcessController::default();

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"STOPPING=1"),
        CONTROL_NS - 2,
    )
    .expect("apply stopping");

    assert_eq!(
        dispatch.notify.applied_fields,
        vec![NotifyAppliedField::Stopping],
    );
    assert!(
        supervisor
            .services()
            .runtime("app")
            .expect("app runtime")
            .stopping_acknowledged
    );

    let mut clock = ScriptedClock::new([CONTROL_NS - 1, CONTROL_NS]);
    supervisor
        .stop_service("app", None, &mut clock)
        .expect("request stop");
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop dispatch");

    assert!(controller.signals.is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Stopping,
    );
    assert_eq!(
        supervisor
            .next_stop_timeout_deadline()
            .expect("stop timeout")
            .due_at_ns,
        CONTROL_NS - 1 + 10_000_000_000,
    );
}

fn shutdown_deadline<'a>(
    supervisor: &'a crate::supervisor::Supervisor,
    service: &str,
) -> &'a ShutdownStopDeadline {
    supervisor
        .shutdown()
        .expect("shutdown")
        .stop_deadlines
        .iter()
        .find(|deadline| deadline.service == service)
        .unwrap_or_else(|| panic!("{service} shutdown deadline"))
}
