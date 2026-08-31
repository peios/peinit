use crate::boundary::ProcessSignal;
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;
use crate::supervisor::tests::TestProcessController;

use super::*;

#[test]
fn signal_reload_sends_sighup_and_advisory_completion_restores_active() {
    let mut supervisor = active_app_supervisor();
    let mut command_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .reload_service("app", None, &mut command_clock)
        .expect("reload app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = accepted.outcome else {
        panic!("expected reload operation");
    };
    let operation_id = operation.returned_operation_id;
    let mut controller = TestProcessController::default();
    let mut control_clock = ScriptedClock::new([CONTROL_NS]);

    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut control_clock)
        .expect("execute reload")
        .expect("reload dispatch");

    assert_eq!(controller.signals[0].signal, ProcessSignal::Sighup);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Reloading,
    );

    let completions = supervisor
        .process_due_reload_detection_windows(CONTROL_NS + 2_000_000_000)
        .expect("complete reload window");

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
        Some("reload signal advisory: detection window expired"),
    );
}

#[test]
fn stop_during_signal_reload_cancels_reload_detection_deadline() {
    let mut supervisor = active_app_supervisor();
    let mut command_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .reload_service("app", None, &mut command_clock)
        .expect("reload app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = accepted.outcome else {
        panic!("expected reload operation");
    };
    let reload_operation_id = operation.returned_operation_id;
    let mut controller = TestProcessController::default();
    let mut control_clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut control_clock)
        .expect("execute reload")
        .expect("reload dispatch");
    assert!(supervisor.next_reload_detection_deadline().is_some());

    let stop_operation_id = stop_app(&mut supervisor);
    let mut stop_clock = ScriptedClock::new([CONTROL_NS + 1]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut stop_clock)
        .expect("execute stop")
        .expect("stop dispatch");

    assert!(supervisor.next_reload_detection_deadline().is_none());
    assert_eq!(
        supervisor
            .operation_status(reload_operation_id)
            .expect("reload operation")
            .state,
        OperationState::Aborted,
    );
    assert_eq!(
        supervisor
            .operation_status(stop_operation_id)
            .expect("stop operation")
            .state,
        OperationState::Running,
    );
    assert_eq!(controller.signals[0].signal, ProcessSignal::Sighup);
    assert_eq!(controller.signals[1].signal, ProcessSignal::Sigterm);
}

#[test]
fn main_process_crash_during_signal_reload_cancels_reload_operation_and_deadline() {
    let mut supervisor = active_app_supervisor();
    let mut command_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .reload_service("app", None, &mut command_clock)
        .expect("reload app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = accepted.outcome else {
        panic!("expected reload operation");
    };
    let operation_id = operation.returned_operation_id;
    let mut controller = TestProcessController::default();
    let mut control_clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut control_clock)
        .expect("execute reload")
        .expect("reload dispatch");
    let main_job = current_app_job(&supervisor);

    let dispatch = supervisor
        .complete_job(main_job, EXIT_NS, 1)
        .expect("complete crashed app");

    assert!(supervisor.next_reload_detection_deadline().is_none());
    assert_ne!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Reloading,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("reload operation")
            .state,
        OperationState::Failed,
    );
    assert_eq!(dispatch.terminal.operation_events.len(), 1);
    assert_eq!(
        dispatch.terminal.operation_events[0].operation_id,
        operation_id,
    );
}

// PEI-359. §5.3: the detection window is two seconds, "fixed and is not
// configurable via the registry". It was clamped to the reload operation's own
// StartTimeout-derived deadline, which made the constant a ceiling rather than
// a value — and made it indirectly registry-configurable through
// `StartTimeout`. A service configured to start fast, which is a reasonable
// thing to do, got a shorter reload window as a consequence, for no reason
// connected to reloading.
#[test]
fn the_reload_detection_window_is_two_seconds_whatever_the_start_timeout() {
    use crate::supervisor::tests::{
        StaticRegistry, TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
    };
    use crate::supervisor::{Supervisor, SupervisorSettings};

    let mut app = alive_service("app");
    app.start_timeout_secs = 1;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut boot_clock = ScriptedClock::new([
        crate::supervisor::tests::BOOT_NS,
        crate::supervisor::tests::APP_LAUNCH_NS,
    ]);
    supervisor
        .run_phase2_boot(&mut registry, &mut boot_clock)
        .expect("boot app");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut boot_clock)
        .expect("launch app")
        .expect("app launch dispatch");
    let mut command_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    supervisor
        .reload_service("app", None, &mut command_clock)
        .expect("reload app");
    let mut controller = TestProcessController::default();
    let mut control_clock = ScriptedClock::new([CONTROL_NS]);

    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut control_clock)
        .expect("execute reload")
        .expect("reload dispatch");

    assert_eq!(
        supervisor
            .next_reload_detection_deadline()
            .expect("reload detection")
            .due_at_ns,
        CONTROL_NS + 2_000_000_000,
    );
}
