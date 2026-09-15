use crate::control::service_security::ServiceAccess;
use crate::service::ServiceSecurityDescriptor;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::supervisor::{Supervisor, SupervisorControlCommandBodyResponse};

use super::super::{LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry, TestProcessController};
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

/// TRM a2 retains a terminal operation for 60 seconds so it can be queried,
/// and §10.2 checks `operation-status` against the target service. When the
/// target has been discarded — a restart aborted because its definition was
/// withdrawn mid-stop is the documented case (§8.2) — there was nothing to
/// check against, and the query answered UNKNOWN_SERVICE for an operation
/// peinit still held (PEI-1076). The right is now checked against the
/// descriptor the service had when the operation was created.
#[test]
fn operation_status_of_a_discarded_service_is_checked_against_its_recorded_descriptor() {
    let recorded = ServiceSecurityDescriptor::RegistryBinary(vec![0x7e, 0x01]);
    let mut app = inactive_alive_service("app");
    app.service_security = recorded.clone();
    let mut supervisor = booted_supervisor(vec![app]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS, LIFECYCLE_COMMAND_NS]);
    let peer = control_peer();
    let operation_id = start_app_operation_id(&mut supervisor, &mut access, &mut clock);
    discard_app(&mut supervisor);
    access.observed_descriptors.clear();

    let request = format!(r#"{{"command":"operation-status","operation_id":"{operation_id}"}}"#);
    let response = supervisor
        .run_checked_control_body_with_response(
            request.as_bytes(),
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("operation status response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        ..
    } = response
    else {
        panic!("expected operation status response, got {response:?}");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["operation"]["id"], operation_id);
    assert_eq!(json["operation"]["service"], "app");
    assert_eq!(access.observed_descriptors, vec![recorded]);
}

#[test]
fn operation_status_of_a_discarded_service_denies_rather_than_reporting_it_unknown() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS, LIFECYCLE_COMMAND_NS]);
    let peer = control_peer();
    let operation_id = start_app_operation_id(&mut supervisor, &mut access, &mut clock);
    discard_app(&mut supervisor);
    let mut access = TestAccessChecker::deny_all_services();

    let request = format!(r#"{{"command":"operation-status","operation_id":"{operation_id}"}}"#);
    let response = supervisor
        .run_checked_control_body_with_response(
            request.as_bytes(),
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("operation status response");

    let SupervisorControlCommandBodyResponse::Rejected { response_line, .. } = response else {
        panic!("expected rejected operation status, got {response:?}");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "ACCESS_DENIED");
    assert_eq!(json["message"], "caller lacks SERVICE_QUERY_STATUS on app");
}

fn start_app_operation_id(
    supervisor: &mut Supervisor,
    access: &mut TestAccessChecker,
    clock: &mut ScriptedClock,
) -> String {
    let mut controller = TestProcessController::default();
    let peer = control_peer();
    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"start","service":"app","wait":false}"#,
            body_context(&peer, access, &mut controller, clock),
        )
        .expect("start response");
    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        ..
    } = response
    else {
        panic!("expected start response");
    };
    response_json(&response_line)["operation_id"]
        .as_str()
        .expect("operation id")
        .to_string()
}

/// Withdraw `app`'s definition while its start is in flight, then let the
/// instance drain: the entry is retained while Starting and discarded on
/// the way out (§3.8), leaving the start operation with no service.
fn discard_app(supervisor: &mut Supervisor) {
    let mut registry = StaticRegistry::services(Vec::new());
    let outcome = supervisor
        .reload_config_from_registry(&mut registry)
        .expect("reload without app");
    assert_eq!(outcome.summary.marked_removed, vec!["app".to_string()]);
    let transition = supervisor
        .services
        .transition_service(
            "app",
            ServiceTransition {
                to: ServiceState::Failed,
                cause: TransitionCause::ProcessCrash,
            },
        )
        .expect("drain app");
    assert!(transition.discarded_definition_removed);
    assert!(supervisor.services().get("app").is_none());
}
