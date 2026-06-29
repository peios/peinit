use crate::control::connection::{
    ControlConnectionRecord, ControlConnectionTable, ControlOperationWait,
};
use crate::control::socket::ControlSocketRead;
use crate::ids::OperationIdAllocator;
use crate::runtime::{RuntimeWorkPumpConfig, RuntimeWorkPumpContext, drain_runtime_work_queues};
use crate::supervisor::{SupervisorControlConnectionTurnContext, SupervisorControlFrameTurn};

use super::super::super::{
    LIFECYCLE_COMMAND_NS, ScriptedClock, TestProcessController, TestProcessLauncher,
    TestTokenProvider, process,
};
use super::super::support::{
    DEFAULT_CONTROL_SECURITY, FakeConnectionIo, TestAccessChecker, booted_supervisor, control_peer,
    inactive_alive_service, response_json,
};

#[test]
fn start_default_wait_registers_connection_wait_and_flushes_on_terminal_operation() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 1,
        LIFECYCLE_COMMAND_NS + 2,
        LIFECYCLE_COMMAND_NS + 3,
        LIFECYCLE_COMMAND_NS + 4,
        LIFECYCLE_COMMAND_NS + 5,
        LIFECYCLE_COMMAND_NS + 6,
        LIFECYCLE_COMMAND_NS + 7,
        LIFECYCLE_COMMAND_NS + 8,
    ]);
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            44,
            ControlConnectionRecord::new(
                FakeConnectionIo::scripted_reads([ControlSocketRead::Bytes(
                    b"{\"command\":\"start\",\"service\":\"app\"}\n".to_vec(),
                )]),
                control_peer(),
            ),
        )
        .expect("admit connection");

    let turn = supervisor
        .process_control_connection_table_turn(
            &mut connections,
            44,
            SupervisorControlConnectionTurnContext {
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: None,
                max_read_bytes: 1024,
                max_request_bytes: crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
                observed_at_ns: 123,
            },
        )
        .expect("connection turn");

    let frame = turn.turn.frame.expect("frame").frame;
    let SupervisorControlFrameTurn::CommandAccepted {
        response_line: None,
        wait: Some(wait),
        ..
    } = frame
    else {
        panic!("expected registered wait");
    };
    assert_eq!(wait.service, "app");
    assert!(
        connections
            .get(44)
            .expect("connection")
            .state()
            .pending_wait()
            .is_some()
    );

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

    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, 123)
        .expect("flush waits");

    assert_eq!(flush.completed.len(), 1);
    let record = connections.get(44).expect("connection");
    assert!(record.state().pending_wait().is_none());
    let writes = record.io().writes.borrow();
    assert_eq!(writes.len(), 1);
    let json = response_json(&writes[0]);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "app");
    assert_eq!(json["state"], "active");
    assert_eq!(
        json["operation_id"],
        wait.operation_id.to_canonical_string()
    );
}

#[test]
fn pending_wait_defers_later_frames_on_same_connection() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            44,
            ControlConnectionRecord::new(
                FakeConnectionIo::scripted_reads([
                    ControlSocketRead::Bytes(
                        b"{\"command\":\"start\",\"service\":\"app\"}\n".to_vec(),
                    ),
                    ControlSocketRead::Bytes(b"{\"command\":\"list\"}\n".to_vec()),
                ]),
                control_peer(),
            ),
        )
        .expect("admit connection");

    let first = supervisor
        .process_control_connection_table_turn(
            &mut connections,
            44,
            SupervisorControlConnectionTurnContext {
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: None,
                max_read_bytes: 1024,
                max_request_bytes: crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
                observed_at_ns: 123,
            },
        )
        .expect("first connection turn");
    assert!(matches!(
        first.turn.frame.expect("frame").frame,
        SupervisorControlFrameTurn::CommandAccepted { wait: Some(_), .. }
    ));

    let second = supervisor
        .process_control_connection_table_turn(
            &mut connections,
            44,
            SupervisorControlConnectionTurnContext {
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: None,
                max_read_bytes: 1024,
                max_request_bytes: crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
                observed_at_ns: 124,
            },
        )
        .expect("second connection turn");

    assert!(second.turn.frame.is_none());
    let record = connections.get(44).expect("connection");
    assert!(record.state().pending_wait().is_some());
    assert_eq!(
        record.state().read_buffer().len(),
        b"{\"command\":\"list\"}\n".len(),
    );
}

#[test]
fn wait_true_lifecycle_command_flushes_operation_timeout_error() {
    let mut app = inactive_alive_service("app");
    app.start_timeout_secs = 1;
    let mut supervisor = booted_supervisor(vec![app]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            44,
            ControlConnectionRecord::new(
                FakeConnectionIo::scripted_reads([ControlSocketRead::Bytes(
                    b"{\"command\":\"start\",\"service\":\"app\"}\n".to_vec(),
                )]),
                control_peer(),
            ),
        )
        .expect("admit connection");

    let turn = supervisor
        .process_control_connection_table_turn(
            &mut connections,
            44,
            SupervisorControlConnectionTurnContext {
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: None,
                max_read_bytes: 1024,
                max_request_bytes: crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
                observed_at_ns: 123,
            },
        )
        .expect("connection turn");

    let frame = turn.turn.frame.expect("frame").frame;
    let SupervisorControlFrameTurn::CommandAccepted {
        response_line: None,
        wait: Some(wait),
        ..
    } = frame
    else {
        panic!("expected registered wait");
    };

    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, LIFECYCLE_COMMAND_NS + 1_000_000_000)
        .expect("flush waits");

    assert_eq!(flush.completed.len(), 1);
    assert!(
        connections
            .get(44)
            .expect("connection")
            .state()
            .pending_wait()
            .is_none()
    );
    let writes = connections
        .get(44)
        .expect("connection")
        .io()
        .writes
        .borrow();
    assert_eq!(writes.len(), 1);
    let json = response_json(&writes[0]);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "OPERATION_TIMEOUT");
    assert!(
        json["message"]
            .as_str()
            .expect("message")
            .contains(&wait.operation_id.to_canonical_string())
    );
}

#[test]
fn stale_pending_wait_flushes_unknown_operation_and_clears_wait() {
    let supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut connections = ControlConnectionTable::new(4);
    let operation_id = OperationIdAllocator::new()
        .allocate_batch(1, LIFECYCLE_COMMAND_NS)
        .expect("operation id")[0];
    connections
        .admit(
            44,
            ControlConnectionRecord::new(FakeConnectionIo::default(), control_peer()),
        )
        .expect("admit connection");
    connections
        .get_mut(44)
        .expect("connection")
        .state_mut()
        .set_pending_wait(ControlOperationWait {
            operation_id,
            service: "app".to_string(),
        });

    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, LIFECYCLE_COMMAND_NS)
        .expect("flush waits");

    assert_eq!(flush.completed.len(), 1);
    let record = connections.get(44).expect("connection");
    assert!(record.state().pending_wait().is_none());
    let writes = record.io().writes.borrow();
    assert_eq!(writes.len(), 1);
    let json = response_json(&writes[0]);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "UNKNOWN_OPERATION");
    assert!(
        json["message"]
            .as_str()
            .expect("message")
            .contains(&operation_id.to_canonical_string())
    );
}
