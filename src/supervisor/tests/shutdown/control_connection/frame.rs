use std::collections::VecDeque;

use crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES;
use crate::control::system::{ControlSecurityDescriptor, SystemAccess};
use crate::control::wire::{ControlConnectionBuffer, ControlFrameRejectReason};
use crate::shutdown::ShutdownKind;
use crate::supervisor::{SupervisorControlFrameTurn, SupervisorSystemShutdownControlBodyError};

use super::super::SHUTDOWN_NS;
use super::super::fixture::shutdown_fixture;
use super::support::{
    TurnAccessCall, TurnAccessChecker, assert_response, control_peer, turn_context,
};
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn shutdown_control_turn_waits_for_complete_frame() {
    let mut supervisor = shutdown_fixture();
    let mut buffer = ControlConnectionBuffer::new();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut access = TurnAccessChecker::allow();

    buffer.append(br#"{"command":"shutdown""#);
    let turn = supervisor
        .process_next_shutdown_control_frame(
            &mut buffer,
            turn_context(
                &control_peer(),
                &mut access,
                &mut controller,
                &mut clock,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("turn");

    assert_eq!(
        turn,
        SupervisorControlFrameTurn::Incomplete {
            buffered_bytes: br#"{"command":"shutdown""#.len(),
        },
    );
    assert!(supervisor.shutdown().is_none());
    assert!(access.calls.is_empty());
}

#[test]
fn shutdown_control_turn_accepts_shutdown_and_preserves_next_frame() {
    let mut supervisor = shutdown_fixture();
    let mut buffer = ControlConnectionBuffer::new();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut access = TurnAccessChecker::allow();

    buffer.append(br#"{"command":"shutdown","type":"reboot"}"#);
    buffer.append(b"\n{\"command\":\"list\"}\n");
    let turn = supervisor
        .process_next_shutdown_control_frame(
            &mut buffer,
            turn_context(
                &control_peer(),
                &mut access,
                &mut controller,
                &mut clock,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("turn");

    let SupervisorControlFrameTurn::ShutdownAccepted {
        response_line,
        dispatch,
        remaining_bytes,
    } = turn
    else {
        panic!("expected accepted shutdown");
    };
    assert_response(&response_line, "ok", None, None);
    assert_eq!(dispatch.command.kind, ShutdownKind::Reboot);
    assert_eq!(remaining_bytes, b"{\"command\":\"list\"}\n".len());
    assert_eq!(buffer.as_slice(), b"{\"command\":\"list\"}\n");
    assert_eq!(
        access.calls,
        vec![TurnAccessCall {
            token_fd: 44,
            descriptor: ControlSecurityDescriptor::Default,
            desired_access: SystemAccess::SHUTDOWN,
        }],
    );
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn shutdown_control_turn_consumes_malformed_complete_frame() {
    let mut supervisor = shutdown_fixture();
    let mut buffer = ControlConnectionBuffer::new();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    buffer.append(b"{\n{\"command\":\"shutdown\",\"type\":\"reboot\"}\n");
    let turn = supervisor
        .process_next_shutdown_control_frame(
            &mut buffer,
            turn_context(
                &control_peer(),
                &mut access,
                &mut controller,
                &mut clock,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("turn");

    let SupervisorControlFrameTurn::ShutdownRejected {
        response_line,
        error,
        remaining_bytes,
    } = turn
    else {
        panic!("expected rejected shutdown frame");
    };
    assert!(matches!(
        error,
        SupervisorSystemShutdownControlBodyError::Parse(_),
    ));
    assert_response(
        &response_line,
        "error",
        Some("MALFORMED_REQUEST"),
        Some("malformed control request"),
    );
    assert_eq!(
        remaining_bytes,
        b"{\"command\":\"shutdown\",\"type\":\"reboot\"}\n".len(),
    );
    assert_eq!(
        buffer.as_slice(),
        b"{\"command\":\"shutdown\",\"type\":\"reboot\"}\n",
    );
    assert!(access.calls.is_empty());
    assert!(supervisor.shutdown().is_none());
}

#[test]
fn shutdown_control_turn_rejects_oversized_frame_and_clears_buffer() {
    let mut supervisor = shutdown_fixture();
    let mut buffer = ControlConnectionBuffer::new();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    buffer.append(b"abcdef");
    let turn = supervisor
        .process_next_shutdown_control_frame(
            &mut buffer,
            turn_context(&control_peer(), &mut access, &mut controller, &mut clock, 5),
        )
        .expect("turn");

    let SupervisorControlFrameTurn::RejectedFrame {
        reason,
        response_line,
        close_after_response,
    } = turn
    else {
        panic!("expected rejected frame");
    };
    assert_eq!(reason, ControlFrameRejectReason::RequestTooLarge);
    assert!(close_after_response);
    assert_response(
        &response_line,
        "error",
        Some("REQUEST_TOO_LARGE"),
        Some("control request too large"),
    );
    assert!(buffer.is_empty());
    assert!(access.calls.is_empty());
    assert!(supervisor.shutdown().is_none());
}

#[test]
fn shutdown_control_turn_denies_without_mutation() {
    let mut supervisor = shutdown_fixture();
    let mut buffer = ControlConnectionBuffer::new();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::deny();

    buffer.append(br#"{"command":"shutdown","type":"poweroff"}"#);
    buffer.append(b"\n");
    let turn = supervisor
        .process_next_shutdown_control_frame(
            &mut buffer,
            turn_context(
                &control_peer(),
                &mut access,
                &mut controller,
                &mut clock,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("turn");

    let SupervisorControlFrameTurn::ShutdownRejected {
        response_line,
        error,
        remaining_bytes,
    } = turn
    else {
        panic!("expected rejected shutdown");
    };
    assert!(matches!(
        error,
        SupervisorSystemShutdownControlBodyError::AccessDenied(_),
    ));
    assert_response(
        &response_line,
        "error",
        Some("ACCESS_DENIED"),
        Some("caller lacks SYSTEM_SHUTDOWN on peinit control"),
    );
    assert_eq!(remaining_bytes, 0);
    assert!(buffer.is_empty());
    assert!(supervisor.shutdown().is_none());
    assert!(controller.signals.is_empty());
    assert!(controller.cgroup_kills.is_empty());
}
