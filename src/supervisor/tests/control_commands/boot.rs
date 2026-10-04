use crate::boot::{BootAttempts, BootMode};
use crate::control::system::SystemAccess;
use crate::control::wire::ControlResponseTimeProjection;
use crate::service::{ErrorControl, ServiceDefinition};
use crate::shutdown::ShutdownKind;
use crate::supervisor::{Supervisor, SupervisorControlCommandBodyResponse, SupervisorSettings};

use super::super::{
    BOOT_NS, ScriptedClock, StaticRegistry, TEST_REALTIME_NS, TestProcessController, settings,
};
use super::support::{
    TestAccessChecker, body_context, booted_supervisor, control_peer, inactive_alive_service,
    response_json,
};

const GRACE_SECS: u32 = 30;

fn booted(mode: BootMode, attempts: BootAttempts, services: Vec<ServiceDefinition>) -> Supervisor {
    let mut supervisor_settings =
        SupervisorSettings::new(crate::boot::phase2::Phase2BootSettings {
            mode,
            boot_success_grace_secs: GRACE_SECS,
            ..settings()
        });
    supervisor_settings.boot_attempts = attempts;
    let mut supervisor = Supervisor::new(supervisor_settings);
    let mut registry = StaticRegistry::services(services);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
}

/// Asks `boot`, with the monotonic clock at `BOOT_NS`.
fn ask_boot(
    supervisor: &mut Supervisor,
    access: &mut TestAccessChecker,
) -> SupervisorControlCommandBodyResponse {
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let peer = control_peer();
    supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"boot"}"#,
            body_context(&peer, access, &mut controller, &mut clock),
        )
        .expect("response")
}

fn answered(response: SupervisorControlCommandBodyResponse) -> serde_json::Value {
    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        dispatch: None,
        ..
    } = response
    else {
        panic!("expected a boot response, got {response:?}");
    };
    response_json(&response_line)
}

#[test]
fn boot_reports_the_mode_the_count_and_what_the_boot_waits_for() {
    // Boot-triggered and Critical, and not yet launched: the boot is
    // waiting for it.
    let mut critical = ServiceDefinition::simple_system_boot("store", "/sbin/store");
    critical.error_control = ErrorControl::Critical;
    let mut supervisor = booted(
        BootMode::Full,
        BootAttempts {
            counted: 2,
            threshold: 3,
        },
        vec![critical],
    );
    let mut access = TestAccessChecker::allow_all();

    let json = answered(ask_boot(&mut supervisor, &mut access));

    assert_eq!(json["status"], "ok");
    assert_eq!(
        json["boot"],
        serde_json::json!({
            "mode": "full",
            "reason": "normal",
            "downgrade": [],
            "attempts": 2,
            "max_attempts": 3,
            "confirmed": false,
            "grace_seconds": GRACE_SECS,
            "waiting_on": ["store"],
            "confirms_at": null,
            "confirm_error": null,
        })
    );
    // A question about the manager, checked against its own descriptor for
    // the one right that changes nothing.
    assert_eq!(
        access.observed_system_desired,
        vec![SystemAccess::QUERY_STATUS]
    );
    assert!(access.observed_desired.is_empty());
}

/// With no Critical services the boot holds from the moment Phase 2 ran,
/// so `boot` can say when it will count.
#[test]
fn boot_says_when_a_boot_waiting_on_nothing_will_count() {
    let mut supervisor = booted(
        BootMode::Full,
        BootAttempts::default(),
        vec![inactive_alive_service("app")],
    );
    let mut access = TestAccessChecker::allow_all();

    let json = answered(ask_boot(&mut supervisor, &mut access));

    assert_eq!(json["boot"]["waiting_on"], serde_json::json!([]));
    // Phase 2 ran at BOOT_NS and the question is asked then too, so the
    // boot counts a whole grace from now.
    let due_ns = BOOT_NS + u64::from(GRACE_SECS) * 1_000_000_000;
    assert_eq!(
        json["boot"]["confirms_at"],
        ControlResponseTimeProjection::new(BOOT_NS, TEST_REALTIME_NS).realtime_timestamp(due_ns)
    );
    assert_eq!(json["boot"]["max_attempts"], 3);
}

#[test]
fn boot_reports_a_safe_boot_that_was_asked_for() {
    let mut supervisor = booted(
        BootMode::Safe,
        BootAttempts::default(),
        vec![inactive_alive_service("app")],
    );
    let mut access = TestAccessChecker::allow_all();

    let json = answered(ask_boot(&mut supervisor, &mut access));

    assert_eq!(json["boot"]["mode"], "safe");
    assert_eq!(json["boot"]["reason"], "requested");
    assert_eq!(json["boot"]["downgrade"], serde_json::json!([]));
}

/// A Full boot downgraded in place names every finding that forced it.
#[test]
fn boot_reports_a_downgrade_and_what_forced_it() {
    let mut critical = ServiceDefinition::simple_system_boot("critical", "/sbin/critical");
    critical.error_control = ErrorControl::Critical;
    critical.requires.push("normal".to_string());
    let mut normal = ServiceDefinition::simple_system_boot("normal", "/sbin/normal");
    normal.requires.push("critical".to_string());
    let mut supervisor = booted(
        BootMode::Full,
        BootAttempts::default(),
        vec![critical, normal],
    );
    let mut access = TestAccessChecker::allow_all();

    let json = answered(ask_boot(&mut supervisor, &mut access));

    assert_eq!(json["boot"]["mode"], "safe");
    assert_eq!(json["boot"]["reason"], "safe_mode_downgrade");
    assert_eq!(
        json["boot"]["downgrade"],
        serde_json::json!(["critical service in dependency cycle critical -> normal"])
    );
}

#[test]
fn boot_is_denied_without_query_status_on_the_control_descriptor() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::deny_system();

    let response = ask_boot(&mut supervisor, &mut access);

    let SupervisorControlCommandBodyResponse::Rejected { response_line, .. } = response else {
        panic!("expected boot to be denied");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "ACCESS_DENIED");
}

/// It changes nothing, so the shutdown gate lets it through, as it does
/// `status` and `list`.
#[test]
fn boot_is_answered_during_shutdown() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, 2_000)
        .expect("begin shutdown");
    let mut access = TestAccessChecker::allow_all();

    let json = answered(ask_boot(&mut supervisor, &mut access));

    assert_eq!(json["status"], "ok");
    assert_eq!(json["boot"]["mode"], "full");
}
