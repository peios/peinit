use crate::control::service_security::ServiceAccess;
use crate::supervisor::SupervisorControlCommandBodyResponse;

use super::super::{ScriptedClock, TestProcessController};
use super::support::{
    TestAccessChecker, body_context, booted_supervisor, control_peer, inactive_alive_service,
    response_json,
};

#[test]
fn status_requires_service_query_access() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::deny_all_services();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"status","service":"app"}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("response");

    let SupervisorControlCommandBodyResponse::Rejected { response_line, .. } = response else {
        panic!("expected rejected status");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "ACCESS_DENIED");
    assert_eq!(json["message"], "caller lacks SERVICE_QUERY_STATUS on app");
}

#[test]
fn reload_config_requires_system_reload_config_access() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::deny_system();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reload-config"}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("response");

    let SupervisorControlCommandBodyResponse::Rejected { response_line, .. } = response else {
        panic!("expected rejected reload-config");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "ACCESS_DENIED");
    assert_eq!(
        json["message"],
        "caller lacks SYSTEM_RELOAD_CONFIG on peinit control",
    );
}

#[test]
fn list_filters_services_without_query_access() {
    let mut supervisor = booted_supervisor(vec![
        inactive_alive_service("app"),
        inactive_alive_service("secret"),
    ]);
    let mut access = TestAccessChecker::allow_only_services(["app"]);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"list"}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        access_denials,
        ..
    } = response
    else {
        panic!("expected accepted list");
    };
    assert_eq!(access_denials.len(), 1);
    assert_eq!(access_denials[0].caller.identity, "admin");
    assert_eq!(access_denials[0].service, "secret");
    assert_eq!(
        access_denials[0].desired_access.bits(),
        ServiceAccess::QUERY_STATUS.bits(),
    );
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    let services = json["services"].as_array().expect("services");
    assert_eq!(services.len(), 1);
    assert_eq!(services[0]["service"], "app");
}
