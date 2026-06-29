use crate::execution::notify::NotifyAppliedField;
use crate::service::runtime::ServiceState;
use crate::supervisor::SupervisorError;
use crate::supervisor::notify::timeout_extension::TimeoutExtensionError;

use super::super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};
use super::{
    CONTROL_NS, NOTIFY_NS, RELOAD_NOTIFY_NS, active_app_supervisor, apply_notify, datagram,
    notify_app_supervisor, reload_app,
};

const START_CAP_EXTENSION_NS: u64 = 90_000_000_000;
const STOP_CAP_EXTENSION_NS: u64 = 30_000_000_000;
const RELOAD_SIGNAL_CAP_EXTENSION_NS: u64 = 117_999_999_999;
const RELOAD_COMMAND_CAP_EXTENSION_NS: u64 = 90_000_000_000;

#[test]
fn starting_extend_timeout_resets_readiness_deadline_and_clamps_to_start_cap() {
    let mut supervisor = notify_app_supervisor();
    let old_deadline = supervisor
        .next_readiness_timeout()
        .expect("readiness timeout");

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=1000000000"),
        NOTIFY_NS,
    )
    .expect("extend start timeout");

    assert_eq!(
        dispatch.notify.applied_fields,
        vec![NotifyAppliedField::ExtendTimeoutUsec {
            value: "1000000000".to_string(),
        }],
    );
    assert_eq!(
        supervisor
            .next_readiness_timeout()
            .expect("readiness timeout")
            .due_at_ns,
        old_deadline.due_at_ns + START_CAP_EXTENSION_NS,
    );
}

#[test]
fn normal_stopping_extend_timeout_resets_stop_deadline_and_clamps_to_stop_cap() {
    let mut supervisor = active_app_supervisor();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS - 1, CONTROL_NS]);
    supervisor
        .stop_service("app", None, &mut clock)
        .expect("request stop");
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop dispatch");
    let old_deadline = supervisor
        .next_stop_timeout_deadline()
        .expect("stop timeout");

    apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=1000000000"),
        CONTROL_NS + 1_000,
    )
    .expect("extend stop timeout");

    assert_eq!(
        supervisor
            .next_stop_timeout_deadline()
            .expect("stop timeout")
            .due_at_ns,
        old_deadline.due_at_ns + STOP_CAP_EXTENSION_NS,
    );
}

#[test]
fn signal_reload_extend_timeout_resets_detection_deadline_and_clamps_to_start_cap() {
    let mut supervisor = active_app_supervisor();
    reload_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload dispatch");
    let old_deadline = supervisor
        .next_reload_detection_deadline()
        .expect("reload detection");

    apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=1000000000"),
        RELOAD_NOTIFY_NS,
    )
    .expect("extend reload timeout");

    assert_eq!(
        supervisor
            .next_reload_detection_deadline()
            .expect("reload detection")
            .due_at_ns,
        old_deadline.due_at_ns + RELOAD_SIGNAL_CAP_EXTENSION_NS,
    );
}

#[test]
fn command_reload_extend_timeout_resets_command_deadline_and_clamps_to_start_cap() {
    let mut supervisor = active_command_reload_supervisor();
    reload_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload dispatch");
    let old_deadline = supervisor
        .next_reload_command_timeout()
        .expect("reload command timeout");

    apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=1000000000"),
        RELOAD_NOTIFY_NS,
    )
    .expect("extend reload command timeout");

    assert_eq!(
        supervisor
            .next_reload_command_timeout()
            .expect("reload command timeout")
            .due_at_ns,
        old_deadline.due_at_ns + RELOAD_COMMAND_CAP_EXTENSION_NS,
    );
}

#[test]
fn malformed_extend_timeout_is_rejected_during_transition() {
    let mut supervisor = notify_app_supervisor();

    let error = apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=not-a-number"),
        NOTIFY_NS,
    )
    .expect_err("invalid extension");

    assert!(matches!(
        error,
        SupervisorError::TimeoutExtension(TimeoutExtensionError::InvalidValue {
            service,
            value,
        }) if service == "app" && value == "not-a-number"
    ));
}

#[test]
fn malformed_extend_timeout_is_ignored_outside_transition() {
    let mut supervisor = active_app_supervisor();

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=not-a-number"),
        NOTIFY_NS,
    )
    .expect("ignored extension");

    assert_eq!(
        dispatch.notify.applied_fields,
        vec![NotifyAppliedField::ExtendTimeoutUsec {
            value: "not-a-number".to_string(),
        }],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
}

fn active_command_reload_supervisor() -> crate::supervisor::Supervisor {
    let mut app = alive_service("app");
    app.exec_reload = Some("/usr/bin/reload".to_string());
    let mut supervisor =
        crate::supervisor::Supervisor::new(crate::supervisor::SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot app");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");
    supervisor
}
