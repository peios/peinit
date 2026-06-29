use std::collections::VecDeque;

use crate::control::connection::ControlConnectionState;
use crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES;
use crate::control::wire::ControlFrameRejectReason;
use crate::supervisor::SupervisorControlFrameTurn;

use super::super::fixture::shutdown_fixture;
use super::support::{TurnAccessChecker, control_peer, turn_context};
use crate::supervisor::tests::shutdown::SHUTDOWN_NS;
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn shutdown_control_connection_frame_enqueues_success_response() {
    let mut supervisor = shutdown_fixture();
    let mut connection = ControlConnectionState::new();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut access = TurnAccessChecker::allow();

    connection
        .read_buffer_mut()
        .append(br#"{"command":"shutdown","type":"halt"}"#);
    connection.read_buffer_mut().append(b"\n");
    let turn = supervisor
        .process_next_shutdown_control_connection_frame(
            &mut connection,
            turn_context(
                &control_peer(),
                &mut access,
                &mut controller,
                &mut clock,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("turn");

    assert!(matches!(
        turn.frame,
        SupervisorControlFrameTurn::ShutdownAccepted { .. },
    ));
    assert_eq!(turn.pending_write_bytes, b"{\"status\":\"ok\"}\n".len());
    assert!(!turn.close_after_write);
    assert_eq!(
        connection.pending_write_bytes(),
        b"{\"status\":\"ok\"}\n".len(),
    );
    assert!(!connection.close_after_write());
    assert!(connection.read_buffer().is_empty());
}

#[test]
fn shutdown_control_connection_frame_reject_marks_close_after_response_flush() {
    let mut supervisor = shutdown_fixture();
    let mut connection = ControlConnectionState::new();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    connection.read_buffer_mut().append(b"abcdef");
    let turn = supervisor
        .process_next_shutdown_control_connection_frame(
            &mut connection,
            turn_context(&control_peer(), &mut access, &mut controller, &mut clock, 5),
        )
        .expect("turn");

    assert!(matches!(
        turn.frame,
        SupervisorControlFrameTurn::RejectedFrame {
            reason: ControlFrameRejectReason::RequestTooLarge,
            ..
        },
    ));
    assert!(turn.close_after_write);
    assert!(connection.close_after_write());
    assert!(connection.pending_write_bytes() > 0);
    assert!(connection.read_buffer().is_empty());
    assert!(supervisor.shutdown().is_none());
}
