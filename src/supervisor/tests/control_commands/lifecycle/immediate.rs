use crate::supervisor::SupervisorControlCommandBodyResponse;

use super::super::super::{LIFECYCLE_COMMAND_NS, ScriptedClock, TestProcessController};
use super::super::support::{
    TestAccessChecker, body_context, booted_supervisor, control_peer, inactive_alive_service,
    response_json,
};

#[test]
fn start_wait_false_returns_lifecycle_ack_immediately() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS, LIFECYCLE_COMMAND_NS]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"start","service":"app","wait":false}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        wait: None,
        ..
    } = response
    else {
        panic!("expected immediate start ack");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "app");
    assert!(json["operation_id"].as_str().is_some());
}

#[test]
fn operation_status_returns_authorized_operation_record() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS, LIFECYCLE_COMMAND_NS]);
    let peer = control_peer();

    let start_response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"start","service":"app","wait":false}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("start response");
    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(start_response_line),
        ..
    } = start_response
    else {
        panic!("expected start response");
    };
    let operation_id = response_json(&start_response_line)["operation_id"]
        .as_str()
        .expect("operation id")
        .to_string();
    let request = format!(r#"{{"command":"operation-status","operation_id":"{operation_id}"}}"#);

    let status_response = supervisor
        .run_checked_control_body_with_response(
            request.as_bytes(),
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("operation status response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        ..
    } = status_response
    else {
        panic!("expected operation status response");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["operation"]["id"], operation_id);
    assert_eq!(json["operation"]["service"], "app");
    assert_eq!(json["operation"]["type"], "start");
    assert_eq!(json["operation"]["state"], "running");
    assert_eq!(
        json["operation"]["requested_at"],
        "2024-05-31T16:08:37.123456789Z"
    );
}
