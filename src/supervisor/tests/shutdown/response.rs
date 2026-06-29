use crate::control::system::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessCheckError,
    SystemAccessCheckRequest, SystemAccessChecker, SystemAccessDecision, SystemAccessDenied,
    SystemShutdownCommandAdmissionError,
};
use crate::security::TokenSummary;
use crate::shutdown::ShutdownKind;
use crate::supervisor::{
    SupervisorError, SupervisorSystemShutdownControlBodyError,
    SupervisorSystemShutdownControlBodyResponse, system_shutdown_control_response_line,
};

use super::super::{ScriptedClock, TestProcessController};
use super::SHUTDOWN_NS;
use super::fixture::shutdown_fixture;

#[test]
fn shutdown_control_success_response_is_ok_line() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);

    let result = supervisor.run_authorized_shutdown_control_body(
        br#"{"command":"shutdown","type":"poweroff"}"#,
        None,
        &mut controller,
        &mut clock,
    );

    assert_response(
        &system_shutdown_control_response_line(result.as_ref()).expect("response"),
        "ok",
        None,
        None,
    );
}

#[test]
fn shutdown_control_parse_error_response_uses_parse_code() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);

    let result =
        supervisor.run_authorized_shutdown_control_body(b"{", None, &mut controller, &mut clock);

    assert_response(
        &system_shutdown_control_response_line(result.as_ref()).expect("response"),
        "error",
        Some("MALFORMED_REQUEST"),
        Some("malformed control request"),
    );
}

#[test]
fn shutdown_control_admission_error_response_uses_client_error_message() {
    let error = SupervisorSystemShutdownControlBodyError::Admission(
        SystemShutdownCommandAdmissionError::InvalidCommand,
    );

    assert_response(
        &system_shutdown_control_response_line(Err(&error)).expect("response"),
        "error",
        Some("INVALID_COMMAND"),
        Some("invalid control command"),
    );
}

#[test]
fn shutdown_control_access_denied_response_names_system_right() {
    let error =
        SupervisorSystemShutdownControlBodyError::AccessDenied(Box::new(SystemAccessDenied {
            caller: TokenSummary::requested_identity("guest"),
            desired_access: SystemAccess::SHUTDOWN,
            granted_access_bits: 0,
        }));

    assert_response(
        &system_shutdown_control_response_line(Err(&error)).expect("response"),
        "error",
        Some("ACCESS_DENIED"),
        Some("caller lacks SYSTEM_SHUTDOWN on peinit control"),
    );
}

#[test]
fn shutdown_control_authorization_failure_is_internal_response() {
    let error = SupervisorSystemShutdownControlBodyError::Authorization(
        SystemAccessCheckError::Boundary("access check failed".to_string()),
    );

    assert_response(
        &system_shutdown_control_response_line(Err(&error)).expect("response"),
        "error",
        Some("INTERNAL_ERROR"),
        Some("control request failed"),
    );
}

#[test]
fn shutdown_control_reentry_response_matches_during_shutdown_contract() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS, SHUTDOWN_NS + 1]);

    supervisor
        .run_authorized_shutdown_control_body(
            br#"{"command":"shutdown","type":"poweroff"}"#,
            None,
            &mut controller,
            &mut clock,
        )
        .expect("first shutdown");
    let result = supervisor.run_authorized_shutdown_control_body(
        br#"{"command":"shutdown","type":"reboot"}"#,
        None,
        &mut controller,
        &mut clock,
    );

    assert_eq!(
        result.as_ref().expect_err("reentry error"),
        &SupervisorSystemShutdownControlBodyError::Supervisor(Box::new(SupervisorError::Shutdown(
            crate::shutdown::ShutdownError::AlreadyInProgress {
                kind: ShutdownKind::Poweroff,
            },
        ))),
    );
    assert_response(
        &system_shutdown_control_response_line(result.as_ref()).expect("response"),
        "error",
        Some("INVALID_STATE"),
        Some("command rejected during shutdown"),
    );
}

#[test]
fn checked_shutdown_control_body_with_response_returns_accepted_dispatch_and_line() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut access = ResponseAccessChecker::allow();

    let response = supervisor
        .run_checked_shutdown_control_body_with_response(
            br#"{"command":"shutdown","type":"halt"}"#,
            &control_peer(),
            &ControlSecurityDescriptor::Default,
            &mut access,
            &mut controller,
            &mut clock,
        )
        .expect("response");

    let SupervisorSystemShutdownControlBodyResponse::Accepted {
        response_line,
        dispatch,
    } = response
    else {
        panic!("expected accepted response");
    };
    assert_response(&response_line, "ok", None, None);
    assert_eq!(dispatch.command.kind, ShutdownKind::Halt);
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn checked_shutdown_control_body_with_response_returns_denied_error_without_mutation() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut access = ResponseAccessChecker::deny();

    let response = supervisor
        .run_checked_shutdown_control_body_with_response(
            br#"{"command":"shutdown","type":"reboot"}"#,
            &control_peer(),
            &ControlSecurityDescriptor::Default,
            &mut access,
            &mut controller,
            &mut clock,
        )
        .expect("response");

    let SupervisorSystemShutdownControlBodyResponse::Rejected {
        response_line,
        error,
    } = response
    else {
        panic!("expected rejected response");
    };
    assert_response(
        &response_line,
        "error",
        Some("ACCESS_DENIED"),
        Some("caller lacks SYSTEM_SHUTDOWN on peinit control"),
    );
    assert!(matches!(
        error,
        SupervisorSystemShutdownControlBodyError::AccessDenied(_),
    ));
    assert!(supervisor.shutdown().is_none());
    assert!(controller.signals.is_empty());
    assert!(controller.cgroup_kills.is_empty());
}

fn assert_response(line: &[u8], status: &str, code: Option<&str>, message: Option<&str>) {
    assert_eq!(line.last(), Some(&b'\n'));
    let response: serde_json::Value =
        serde_json::from_slice(&line[..line.len() - 1]).expect("response json");
    assert_eq!(response["status"], status);
    match code {
        Some(code) => assert_eq!(response["code"], code),
        None => assert!(response.get("code").is_none()),
    }
    match message {
        Some(message) => assert_eq!(response["message"], message),
        None => assert!(response.get("message").is_none()),
    }
}

#[derive(Debug, Clone, Copy)]
struct ResponseAccessChecker {
    decision: SystemAccessDecision,
}

impl ResponseAccessChecker {
    fn allow() -> Self {
        Self {
            decision: SystemAccessDecision {
                allowed: true,
                granted_access_bits: SystemAccess::ALL.bits(),
            },
        }
    }

    fn deny() -> Self {
        Self {
            decision: SystemAccessDecision {
                allowed: false,
                granted_access_bits: 0,
            },
        }
    }
}

impl SystemAccessChecker for ResponseAccessChecker {
    fn check_system_access(
        &mut self,
        _request: SystemAccessCheckRequest<'_>,
    ) -> Result<SystemAccessDecision, SystemAccessCheckError> {
        Ok(self.decision)
    }
}

fn control_peer() -> ControlPeer {
    ControlPeer::borrowed_token_fd(44, TokenSummary::requested_identity("admin"))
}
