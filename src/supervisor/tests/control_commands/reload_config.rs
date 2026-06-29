use std::os::unix::net::UnixStream;

use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlPeer;
use crate::control::system::ControlSecurityDescriptor;
use crate::service::ServiceEnvironmentVariable;
use crate::shutdown::ShutdownKind;
use crate::supervisor::{
    SupervisorControlCommandBodyContext, SupervisorControlCommandBodyResponse,
};

use super::super::{ScriptedClock, StaticRegistry, TestProcessController};
use super::support::{
    DEFAULT_CONTROL_SECURITY, TestAccessChecker, booted_supervisor, inactive_alive_service,
    response_json,
};

#[test]
fn reload_config_uses_registry_client_and_returns_summary() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut registry = StaticRegistry::services(vec![
        inactive_alive_service("app"),
        inactive_alive_service("new"),
    ]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([2_001]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reload-config"}"#,
            SupervisorControlCommandBodyContext {
                peer: &peer,
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: Some(&mut registry),
            },
        )
        .expect("response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        ..
    } = response
    else {
        panic!("expected reload-config response");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["summary"]["added"], serde_json::json!(["new"]));
    assert!(supervisor.services().get("new").is_some());
}

#[test]
fn reload_config_updates_live_control_security_and_limits() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let limits = ControlSocketLimits {
        max_connections: 4,
        max_request_bytes: 2048,
        connection_timeout_secs: 5,
    };
    let mut registry = StaticRegistry::services(vec![inactive_alive_service("app")])
        .with_control_config(
            ControlSecurityDescriptor::RegistryBinary(vec![1, 2]),
            limits,
        );
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reload-config"}"#,
            SupervisorControlCommandBodyContext {
                peer: &peer,
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: Some(&mut registry),
            },
        )
        .expect("response");

    assert_eq!(
        supervisor.control_security(),
        &ControlSecurityDescriptor::RegistryBinary(vec![1, 2]),
    );
    assert_eq!(supervisor.control_limits(), limits);
}

#[test]
fn reload_config_discards_fd_store_for_removed_inactive_service() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let (left, _right) = UnixStream::pair().expect("socket pair");
    assert_eq!(
        supervisor.fd_store.store(
            "app",
            1,
            crate::fd_store::StoreFdRequest {
                name: Some("listener".to_string()),
                poll: true,
                fd: left.into(),
            },
        ),
        crate::fd_store::StoreFdOutcome::Stored,
    );
    let mut registry = StaticRegistry::services(Vec::new());
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reload-config"}"#,
            SupervisorControlCommandBodyContext {
                peer: &peer,
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: Some(&mut registry),
            },
        )
        .expect("response");

    assert!(supervisor.services().get("app").is_none());
    assert!(supervisor.fd_store().service("app").is_none());
}

#[test]
fn reload_config_updates_global_environment_snapshot() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut registry = StaticRegistry::services_with_global_environment(
        vec![inactive_alive_service("app")],
        vec![ServiceEnvironmentVariable {
            name: "GLOBAL_ONLY".to_string(),
            value: "yes".to_string(),
        }],
    );
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reload-config"}"#,
            SupervisorControlCommandBodyContext {
                peer: &peer,
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: Some(&mut registry),
            },
        )
        .expect("response");

    assert_eq!(
        supervisor.global_environment(),
        &[ServiceEnvironmentVariable {
            name: "GLOBAL_ONLY".to_string(),
            value: "yes".to_string(),
        }],
    );
}

#[test]
fn reload_config_updates_eventd_log_socket_path() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut registry = StaticRegistry::services(vec![inactive_alive_service("app")])
        .with_eventd_log_socket_path("/run/peinit/eventd.sock");
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reload-config"}"#,
            SupervisorControlCommandBodyContext {
                peer: &peer,
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: Some(&mut registry),
            },
        )
        .expect("response");

    assert_eq!(
        supervisor.eventd_log_socket_path(),
        Some("/run/peinit/eventd.sock"),
    );
}

#[test]
fn reload_config_updates_shutdown_timeout_setting() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut registry = StaticRegistry::services(vec![inactive_alive_service("app")])
        .with_shutdown_timeout_secs(34);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reload-config"}"#,
            SupervisorControlCommandBodyContext {
                peer: &peer,
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: Some(&mut registry),
            },
        )
        .expect("response");

    assert_eq!(supervisor.settings().shutdown.global_timeout_secs, 34);
}

#[test]
fn reload_config_is_rejected_during_shutdown_without_reading_registry() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, 2_000)
        .expect("begin shutdown");
    let mut registry = StaticRegistry::services(vec![
        inactive_alive_service("app"),
        inactive_alive_service("fresh"),
    ]);
    let mut access = TestAccessChecker::allow_all();
    let mut clock = ScriptedClock::new([]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reload-config"}"#,
            SupervisorControlCommandBodyContext {
                peer: &peer,
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: Some(&mut registry),
            },
        )
        .expect("response");

    let SupervisorControlCommandBodyResponse::Rejected { response_line, .. } = response else {
        panic!("expected reload-config rejection during shutdown");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "INVALID_STATE");
    assert_eq!(json["message"], "command rejected during shutdown");
    assert!(supervisor.services().get("fresh").is_none());
}

#[test]
fn status_query_is_allowed_during_shutdown() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, 2_000)
        .expect("begin shutdown");
    let mut access = TestAccessChecker::allow_all();
    let mut clock = ScriptedClock::new([2_001]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"status","service":"app"}"#,
            SupervisorControlCommandBodyContext {
                peer: &peer,
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: None,
            },
        )
        .expect("response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        ..
    } = response
    else {
        panic!("expected status response during shutdown");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "app");
}

fn control_peer() -> ControlPeer {
    super::support::control_peer()
}
