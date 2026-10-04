use crate::control::service_security::ServiceAccess;
use crate::runtime::{RuntimeWorkPumpConfig, RuntimeWorkPumpContext, drain_runtime_work_queues};
use crate::service::ServiceSecurityDescriptor;
use crate::supervisor::SupervisorControlCommandBodyResponse;

use super::super::{
    LIFECYCLE_COMMAND_NS, ScriptedClock, TestProcessController, TestProcessLauncher,
    TestTokenProvider, process,
};
use super::support::{
    TestAccessChecker, body_context, booted_supervisor, control_peer, inactive_alive_service,
    response_json,
};

#[test]
fn status_projects_current_job_time_and_uptime_from_realtime_clock() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 5_000_000_000,
    ]);
    let peer = control_peer();

    let start_response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"start","service":"app","wait":false}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("start response");
    assert!(matches!(
        start_response,
        SupervisorControlCommandBodyResponse::Accepted { .. }
    ));

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9000, 90)]);
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default();
    drain_runtime_work_queues(
        &mut supervisor,
        &mut RuntimeWorkPumpContext {
            clock: &mut clock,
            controller: &mut controller,
            token_provider: &mut tokens,
            process_launcher: &mut launcher,
            filesystem_check_launcher: &mut filesystem_check_launcher,
            config: RuntimeWorkPumpConfig::default(),
        },
    )
    .expect("drain work");

    let status_response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"status","service":"app"}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("status response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        ..
    } = status_response
    else {
        panic!("expected status response");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "app");
    assert_eq!(json["state"], "active");
    assert_eq!(json["uptime_seconds"], 5);
    assert!(json["warnings"].as_array().expect("warnings").is_empty());
    assert_eq!(
        json["current_job"]["started_at"],
        "2024-05-31T16:08:32.123456789Z"
    );
}

/// PSPU §4.14: `status` says what the caller may do, by asking the access
/// checker for MAXIMUM_ALLOWED against the service's descriptor once the
/// query itself is allowed, and reporting the service rights it granted.
#[test]
fn status_reports_the_rights_a_maximum_allowed_check_grants() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access =
        TestAccessChecker::granting(ServiceAccess::QUERY_STATUS.union(ServiceAccess::STOP));
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"status","service":"app"}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("status response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        access_denials,
        ..
    } = response
    else {
        panic!("expected status response, got {response:?}");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["granted"], serde_json::json!(["query_status", "stop"]));
    assert_eq!(
        access.observed_desired,
        vec![ServiceAccess::QUERY_STATUS, ServiceAccess::MAXIMUM_ALLOWED]
    );
    // Both checks were against the service's own descriptor.
    assert_eq!(
        access.observed_descriptors,
        vec![
            ServiceSecurityDescriptor::Default,
            ServiceSecurityDescriptor::Default
        ]
    );
    // Not holding START is not a denial: nothing was refused.
    assert!(access_denials.is_empty());
}

/// A lifecycle command with nothing to do answers with the status shape
/// (§4.12), and that carries `granted` for its caller too.
#[test]
fn a_command_answered_with_the_status_carries_granted() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access =
        TestAccessChecker::granting(ServiceAccess::QUERY_STATUS.union(ServiceAccess::STOP));
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS, LIFECYCLE_COMMAND_NS]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"stop","service":"app","wait":false}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("stop response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        ..
    } = response
    else {
        panic!("expected a status-shaped answer, got {response:?}");
    };
    let json = response_json(&response_line);
    assert_eq!(json["state"], "inactive");
    assert!(
        json.get("operation_id").is_none(),
        "status shape, not an ack"
    );
    assert_eq!(json["granted"], serde_json::json!(["query_status", "stop"]));
    assert_eq!(
        access.observed_desired,
        vec![ServiceAccess::STOP, ServiceAccess::MAXIMUM_ALLOWED]
    );
}
