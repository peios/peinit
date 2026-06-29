use std::collections::VecDeque;

use crate::control::connection::{
    ControlConnectionRecord, ControlConnectionTable, ControlConnectionWriteTurn,
};
use crate::control::socket::{
    ControlSocketRead, ControlSocketWrite, DEFAULT_MAX_REQUEST_SIZE_BYTES,
};
use crate::supervisor::{
    SupervisorControlFrameTurn, SupervisorShutdownControlConnectionTableTurnError,
};

use super::super::fixture::shutdown_fixture;
use super::support::{FakeConnectionIo, TurnAccessChecker, connection_turn_context, control_peer};
use crate::supervisor::tests::shutdown::SHUTDOWN_NS;
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn shutdown_connection_table_turn_retains_live_shutdown_connection() {
    let request = b"{\"command\":\"shutdown\",\"type\":\"reboot\"}\n";
    let mut supervisor = shutdown_fixture();
    let mut connections = single_connection_table(
        10,
        FakeConnectionIo::scripted(
            [ControlSocketRead::Bytes(request.to_vec())],
            [ControlSocketWrite::Complete],
        ),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut access = TurnAccessChecker::allow();

    let table_turn = supervisor
        .process_shutdown_control_connection_table_turn(
            &mut connections,
            10,
            connection_turn_context(
                &mut access,
                &mut controller,
                &mut clock,
                1024,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("table turn");

    assert_eq!(table_turn.fd, 10);
    assert!(!table_turn.removed);
    assert_eq!(table_turn.active_connections, 1);
    assert!(connections.get(10).is_some());
    assert!(matches!(
        table_turn.turn.frame.expect("frame").frame,
        SupervisorControlFrameTurn::ShutdownAccepted { .. },
    ));
    assert_eq!(connections.get(10).expect("record").io().writes.len(), 1);
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn shutdown_connection_table_turn_removes_eof_connection() {
    let mut supervisor = shutdown_fixture();
    let mut connections =
        single_connection_table(11, FakeConnectionIo::scripted([ControlSocketRead::Eof], []));
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    let table_turn = supervisor
        .process_shutdown_control_connection_table_turn(
            &mut connections,
            11,
            connection_turn_context(
                &mut access,
                &mut controller,
                &mut clock,
                1024,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect("table turn");

    assert!(table_turn.removed);
    assert_eq!(table_turn.active_connections, 0);
    assert!(connections.is_empty());
    assert!(table_turn.turn.frame.is_none());
}

#[test]
fn shutdown_connection_table_turn_removes_after_flushed_terminal_error() {
    let mut supervisor = shutdown_fixture();
    let mut connections = single_connection_table(
        12,
        FakeConnectionIo::scripted(
            [ControlSocketRead::Bytes(b"abcdef".to_vec())],
            [ControlSocketWrite::Complete],
        ),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    let table_turn = supervisor
        .process_shutdown_control_connection_table_turn(
            &mut connections,
            12,
            connection_turn_context(&mut access, &mut controller, &mut clock, 1024, 5),
        )
        .expect("table turn");

    assert!(matches!(
        table_turn.turn.frame.expect("frame").frame,
        SupervisorControlFrameTurn::RejectedFrame { .. },
    ));
    assert!(matches!(
        table_turn.turn.write,
        ControlConnectionWriteTurn::Complete {
            close_after_write: true,
            ..
        },
    ));
    assert!(table_turn.removed);
    assert_eq!(table_turn.active_connections, 0);
    assert!(connections.is_empty());
}

#[test]
fn shutdown_connection_table_turn_retains_until_terminal_error_is_fully_flushed() {
    let mut supervisor = shutdown_fixture();
    let mut connections = single_connection_table(
        13,
        FakeConnectionIo::scripted(
            [ControlSocketRead::Bytes(b"abcdef".to_vec())],
            [ControlSocketWrite::Partial { written: 5 }],
        ),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    let table_turn = supervisor
        .process_shutdown_control_connection_table_turn(
            &mut connections,
            13,
            connection_turn_context(&mut access, &mut controller, &mut clock, 1024, 5),
        )
        .expect("table turn");

    assert!(!table_turn.removed);
    assert_eq!(table_turn.active_connections, 1);
    let record = connections.get(13).expect("retained record");
    assert!(record.state().close_after_write());
    assert!(record.state().pending_write_bytes() > 0);
}

#[test]
fn shutdown_connection_table_turn_reports_missing_fd_without_mutation() {
    let mut supervisor = shutdown_fixture();
    let mut connections = single_connection_table(
        14,
        FakeConnectionIo::scripted([ControlSocketRead::WouldBlock], []),
    );
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(VecDeque::new());
    let mut access = TurnAccessChecker::allow();

    let err = supervisor
        .process_shutdown_control_connection_table_turn(
            &mut connections,
            99,
            connection_turn_context(
                &mut access,
                &mut controller,
                &mut clock,
                1024,
                DEFAULT_MAX_REQUEST_SIZE_BYTES,
            ),
        )
        .expect_err("missing connection");

    assert!(matches!(
        err,
        SupervisorShutdownControlConnectionTableTurnError::MissingConnection { fd: 99 },
    ));
    assert_eq!(connections.len(), 1);
}

fn single_connection_table(
    fd: i32,
    io: FakeConnectionIo,
) -> ControlConnectionTable<ControlConnectionRecord<FakeConnectionIo>> {
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(fd, ControlConnectionRecord::new(io, control_peer()))
        .expect("seed connection");
    connections
}
