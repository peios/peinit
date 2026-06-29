use std::collections::VecDeque;

use crate::control::connection::{
    ControlConnectionReadTurn, ControlConnectionRecord, ControlConnectionWriteTurn,
};
use crate::control::socket::{
    ControlSocketRead, ControlSocketWrite, DEFAULT_MAX_REQUEST_SIZE_BYTES,
};
use crate::supervisor::SupervisorControlFrameTurn;

use super::super::fixture::shutdown_fixture;
use super::support::{
    FakeConnectionIo, TurnAccessChecker, assert_response, connection_turn_context, control_peer,
};
use crate::supervisor::tests::shutdown::SHUTDOWN_NS;
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn shutdown_connection_turn_reads_accepts_shutdown_and_flushes_response() {
    let request = b"{\"command\":\"shutdown\",\"type\":\"reboot\"}\n";
    let mut supervisor = shutdown_fixture();
    let mut connection = ControlConnectionRecord::new(
        FakeConnectionIo::scripted(
            [ControlSocketRead::Bytes(request.to_vec())],
            [ControlSocketWrite::Complete],
        ),
        control_peer(),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut access = TurnAccessChecker::allow();

    let turn = supervisor
        .process_shutdown_control_connection_turn(
            &mut connection,
            connection_turn_context(
                &mut access,
                &mut controller,
                &mut clock,
                1024,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("turn");

    assert_eq!(
        turn.read,
        ControlConnectionReadTurn::Bytes {
            read_bytes: request.len(),
            buffered_bytes: request.len(),
        },
    );
    assert!(matches!(
        turn.frame.expect("frame").frame,
        SupervisorControlFrameTurn::ShutdownAccepted { .. },
    ));
    assert_eq!(
        turn.write,
        ControlConnectionWriteTurn::Complete {
            written: b"{\"status\":\"ok\"}\n".len(),
            close_after_write: false,
        },
    );
    assert!(!turn.close_connection);
    assert_eq!(connection.state().pending_write_bytes(), 0);
    assert_response(&connection.io().writes[0], "ok", None, None);
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn shutdown_connection_turn_keeps_connection_open_after_incomplete_request() {
    let request = b"{\"command\":\"shutdown\"";
    let mut supervisor = shutdown_fixture();
    let mut connection = ControlConnectionRecord::new(
        FakeConnectionIo::scripted([ControlSocketRead::Bytes(request.to_vec())], []),
        control_peer(),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([]);
    let mut access = TurnAccessChecker::allow();

    let turn = supervisor
        .process_shutdown_control_connection_turn(
            &mut connection,
            connection_turn_context(
                &mut access,
                &mut controller,
                &mut clock,
                1024,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("turn");

    assert!(matches!(
        turn.frame.expect("frame").frame,
        SupervisorControlFrameTurn::Incomplete { .. },
    ));
    assert_eq!(
        turn.write,
        ControlConnectionWriteTurn::Idle {
            close_after_write: false,
        },
    );
    assert!(!turn.close_connection);
    assert_eq!(connection.state().read_buffer().as_slice(), request);
    assert!(connection.io().writes.is_empty());
    assert!(supervisor.shutdown().is_none());
}

#[test]
fn shutdown_connection_turn_closes_after_oversized_error_is_flushed() {
    let mut supervisor = shutdown_fixture();
    let mut connection = ControlConnectionRecord::new(
        FakeConnectionIo::scripted(
            [ControlSocketRead::Bytes(b"abcdef".to_vec())],
            [ControlSocketWrite::Complete],
        ),
        control_peer(),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    let turn = supervisor
        .process_shutdown_control_connection_turn(
            &mut connection,
            connection_turn_context(&mut access, &mut controller, &mut clock, 1024, 5),
        )
        .expect("turn");

    assert!(matches!(
        turn.frame.expect("frame").frame,
        SupervisorControlFrameTurn::RejectedFrame { .. },
    ));
    assert!(matches!(
        turn.write,
        ControlConnectionWriteTurn::Complete {
            close_after_write: true,
            ..
        },
    ));
    assert!(turn.close_connection);
    assert_eq!(connection.state().pending_write_bytes(), 0);
    assert_response(
        &connection.io().writes[0],
        "error",
        Some("REQUEST_TOO_LARGE"),
        Some("control request too large"),
    );
    assert!(supervisor.shutdown().is_none());
}

#[test]
fn shutdown_connection_turn_defers_close_when_error_response_is_partially_written() {
    let mut supervisor = shutdown_fixture();
    let mut connection = ControlConnectionRecord::new(
        FakeConnectionIo::scripted(
            [ControlSocketRead::Bytes(b"abcdef".to_vec())],
            [ControlSocketWrite::Partial { written: 5 }],
        ),
        control_peer(),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    let turn = supervisor
        .process_shutdown_control_connection_turn(
            &mut connection,
            connection_turn_context(&mut access, &mut controller, &mut clock, 1024, 5),
        )
        .expect("turn");

    assert!(matches!(
        turn.write,
        ControlConnectionWriteTurn::Partial { written: 5, .. },
    ));
    assert!(!turn.close_connection);
    assert!(connection.state().close_after_write());
    assert!(connection.state().pending_write_bytes() > 0);
}

#[test]
fn shutdown_connection_turn_closes_on_eof_without_processing_frame() {
    let mut supervisor = shutdown_fixture();
    let mut connection = ControlConnectionRecord::new(
        FakeConnectionIo::scripted([ControlSocketRead::Eof], []),
        control_peer(),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    let turn = supervisor
        .process_shutdown_control_connection_turn(
            &mut connection,
            connection_turn_context(
                &mut access,
                &mut controller,
                &mut clock,
                1024,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("turn");

    assert_eq!(turn.read, ControlConnectionReadTurn::Eof);
    assert!(turn.frame.is_none());
    assert_eq!(
        turn.write,
        ControlConnectionWriteTurn::Idle {
            close_after_write: false,
        },
    );
    assert!(turn.close_connection);
    assert!(access.calls.is_empty());
    assert!(supervisor.shutdown().is_none());
}
