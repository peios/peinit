use crate::runtime::RuntimeShutdownEventTurn;
use crate::service::ErrorControl;
use crate::supervisor::tests::{
    APP_CRASH_NS, BOOT_NS, ScriptedClock, TestProcessLauncher, TestTokenProvider, alive_service,
    process, settings,
};
use crate::supervisor::{
    Supervisor, SupervisorLifecycleDeadlineKind, SupervisorLifecycleDeadlineTimerTurn,
    SupervisorSettings,
};

use super::lifecycle_deadline::support::process_expired_lifecycle_deadline_with_counter_result;
use super::support::DeadlineTimerCall;

fn booted_critical_alive_app_supervisor(grace_secs: u32, satisfied_at_ns: u64) -> Supervisor {
    let mut app = alive_service("app");
    app.error_control = ErrorControl::Critical;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(
        crate::boot::phase2::Phase2BootSettings {
            boot_success_grace_secs: grace_secs,
            ..settings()
        },
    ));
    let mut registry = crate::supervisor::tests::StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, satisfied_at_ns]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot critical app");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch critical app")
        .expect("launch dispatch");
    supervisor
}

fn booted_noncritical_app_supervisor(grace_secs: u32) -> Supervisor {
    let app = alive_service("app");
    let mut supervisor = Supervisor::new(SupervisorSettings::new(
        crate::boot::phase2::Phase2BootSettings {
            boot_success_grace_secs: grace_secs,
            ..settings()
        },
    ));
    let mut registry = crate::supervisor::tests::StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot noncritical app");
    supervisor
}

#[test]
fn runtime_lifecycle_deadline_timer_resets_boot_attempt_counter_after_critical_grace() {
    const GRACE_SECS: u32 = 2;
    const DUE_NS: u64 = APP_CRASH_NS + (GRACE_SECS as u64 * 1_000_000_000);

    let mut supervisor = booted_critical_alive_app_supervisor(GRACE_SECS, APP_CRASH_NS);

    let deadline = supervisor.next_lifecycle_deadline().expect("deadline");
    assert_eq!(deadline.due_at_ns, DUE_NS);
    assert_eq!(deadline.kind, SupervisorLifecycleDeadlineKind::BootSuccess);

    let (turn, lifecycle_timer, _controller, boot_attempt_counter) =
        process_expired_lifecycle_deadline_with_counter_result(&mut supervisor, DUE_NS, Ok(()));

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive),
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn::Disarmed,
        ..
    } = turn
    else {
        panic!("expected boot-success lifecycle deadline turn");
    };
    assert_eq!(drive.boot_successes.len(), 1);
    assert_eq!(
        drive.boot_successes[0].required_critical_services,
        vec!["app".to_string()],
    );
    assert_eq!(drive.boot_successes[0].satisfied_since_ns, APP_CRASH_NS);
    assert_eq!(drive.boot_successes[0].due_at_ns, DUE_NS);
    assert_eq!(drive.boot_successes[0].reset_result, Ok(()));
    assert_eq!(boot_attempt_counter.reset_calls, 1);
    assert_eq!(
        lifecycle_timer.calls,
        vec![DeadlineTimerCall::Read, DeadlineTimerCall::Disarm],
    );
    assert!(supervisor.next_lifecycle_deadline().is_none());
}

#[test]
fn boot_success_counter_resets_after_grace_when_no_critical_services_are_required() {
    const GRACE_SECS: u32 = 2;
    const DUE_NS: u64 = BOOT_NS + (GRACE_SECS as u64 * 1_000_000_000);

    let mut supervisor = booted_noncritical_app_supervisor(GRACE_SECS);

    let deadline = supervisor.next_lifecycle_deadline().expect("deadline");
    assert_eq!(deadline.due_at_ns, DUE_NS);
    assert_eq!(deadline.kind, SupervisorLifecycleDeadlineKind::BootSuccess);

    let (turn, _lifecycle_timer, _controller, boot_attempt_counter) =
        process_expired_lifecycle_deadline_with_counter_result(&mut supervisor, DUE_NS, Ok(()));

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive), ..
    } = turn
    else {
        panic!("expected boot-success lifecycle deadline turn");
    };
    assert_eq!(drive.boot_successes.len(), 1);
    assert!(
        drive.boot_successes[0]
            .required_critical_services
            .is_empty()
    );
    assert_eq!(drive.boot_successes[0].satisfied_since_ns, BOOT_NS);
    assert_eq!(drive.boot_successes[0].due_at_ns, DUE_NS);
    assert_eq!(boot_attempt_counter.reset_calls, 1);
}

#[test]
fn boot_success_counter_reset_write_failure_is_nonfatal() {
    const GRACE_SECS: u32 = 2;
    const DUE_NS: u64 = APP_CRASH_NS + (GRACE_SECS as u64 * 1_000_000_000);

    let mut supervisor = booted_critical_alive_app_supervisor(GRACE_SECS, APP_CRASH_NS);
    let reset_error = crate::boundary::BoundaryError::Recovery("disk full".to_string());

    let (turn, _lifecycle_timer, _controller, boot_attempt_counter) =
        process_expired_lifecycle_deadline_with_counter_result(
            &mut supervisor,
            DUE_NS,
            Err(reset_error.clone()),
        );

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive), ..
    } = turn
    else {
        panic!("expected boot-success lifecycle deadline turn");
    };
    assert_eq!(drive.boot_successes.len(), 1);
    assert_eq!(drive.boot_successes[0].reset_result, Err(reset_error));
    assert_eq!(boot_attempt_counter.reset_calls, 1);
    assert!(supervisor.next_lifecycle_deadline().is_none());

    // `boot` does not call it confirmed: the counter still holds this boot,
    // so the next one will count it as a failure. It says why instead.
    let status = supervisor.boot_status();
    assert!(!status.confirmed);
    assert!(status.waiting_on.is_empty());
    assert_eq!(status.confirms_at_ns, None);
    assert_eq!(
        status.confirm_error.as_deref(),
        Some("Recovery(\"disk full\")")
    );
}

/// `boot` names the Critical services a boot is waiting for, then, once
/// they all hold, when it will count — and after the reset, that it has.
#[test]
fn boot_status_follows_the_boot_to_its_confirmation() {
    const GRACE_SECS: u32 = 2;
    const DUE_NS: u64 = APP_CRASH_NS + (GRACE_SECS as u64 * 1_000_000_000);

    let mut app = alive_service("app");
    app.error_control = ErrorControl::Critical;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(
        crate::boot::phase2::Phase2BootSettings {
            boot_success_grace_secs: GRACE_SECS,
            ..settings()
        },
    ));
    let mut registry = crate::supervisor::tests::StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_CRASH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot critical app");

    let waiting = supervisor.boot_status();
    assert_eq!(waiting.mode, crate::boot::BootMode::Full);
    assert_eq!(waiting.reason, crate::boot::BootModeReason::Normal);
    assert!(!waiting.confirmed);
    assert_eq!(waiting.grace_seconds, GRACE_SECS);
    assert_eq!(waiting.waiting_on, vec!["app".to_string()]);
    assert_eq!(waiting.confirms_at_ns, None);

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch critical app")
        .expect("launch dispatch");

    let holding = supervisor.boot_status();
    assert!(!holding.confirmed);
    assert!(holding.waiting_on.is_empty());
    assert_eq!(holding.confirms_at_ns, Some(DUE_NS));

    process_expired_lifecycle_deadline_with_counter_result(&mut supervisor, DUE_NS, Ok(()));

    let confirmed = supervisor.boot_status();
    assert!(confirmed.confirmed);
    assert!(confirmed.waiting_on.is_empty());
    assert_eq!(confirmed.confirms_at_ns, None);
    assert_eq!(confirmed.confirm_error, None);
}
