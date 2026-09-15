use crate::control::connection::{
    ControlConnectionReadTurn, ControlConnectionRecord, ControlConnectionTable,
    ControlOperationWait, ControlPendingWait,
};
use crate::control::socket::ControlSocketRead;
use crate::ids::OperationIdAllocator;
use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::runtime::{RuntimeWorkPumpConfig, RuntimeWorkPumpContext, drain_runtime_work_queues};
use crate::service::Readiness;
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

    let frame = turn.turn.frames.into_iter().next().expect("frame").frame;
    let SupervisorControlFrameTurn::CommandAccepted {
        response_line: None,
        wait: Some(wait),
        ..
    } = frame
    else {
        panic!("expected registered wait");
    };
    let wait = wait.operation().expect("operation wait").clone();
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
        .flush_terminal_control_waits(&mut connections, 123, 0)
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
        first.turn.frames.into_iter().next().expect("frame").frame,
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

    assert!(second.turn.frames.is_empty());
    let record = connections.get(44).expect("connection");
    assert!(record.state().pending_wait().is_some());
    assert_eq!(
        record.state().read_buffer().len(),
        b"{\"command\":\"list\"}\n".len(),
    );
}

#[test]
fn two_frames_in_one_read_are_both_answered_in_order() {
    let mut supervisor = booted_supervisor(vec![
        inactive_alive_service("app"),
        inactive_alive_service("db"),
    ]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS, LIFECYCLE_COMMAND_NS + 1]);
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            44,
            ControlConnectionRecord::new(
                FakeConnectionIo::scripted_reads([ControlSocketRead::Bytes(
                    b"{\"command\":\"status\",\"service\":\"app\"}\n\
                      {\"command\":\"status\",\"service\":\"db\"}\n"
                        .to_vec(),
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

    assert_eq!(turn.turn.frames.len(), 2);
    assert!(turn.turn.frames.iter().all(|frame| matches!(
        frame.frame,
        SupervisorControlFrameTurn::CommandAccepted { .. }
    )));
    let record = connections.get(44).expect("connection");
    assert!(record.state().read_buffer().is_empty());
    let writes = record.io().writes.borrow();
    let written = writes.concat();
    let responses = written
        .split_inclusive(|byte| *byte == b'\n')
        .map(response_json)
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["status"], "ok");
    assert_eq!(responses[0]["service"], "app");
    assert_eq!(responses[1]["status"], "ok");
    assert_eq!(responses[1]["service"], "db");
}

#[test]
fn frame_buffered_behind_wait_is_answered_once_the_wait_clears() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(
        (0..16)
            .map(|offset| LIFECYCLE_COMMAND_NS + offset)
            .collect::<Vec<_>>(),
    );
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            44,
            ControlConnectionRecord::new(
                FakeConnectionIo::scripted_reads([ControlSocketRead::Bytes(
                    b"{\"command\":\"start\",\"service\":\"app\"}\n\
                      {\"command\":\"status\",\"service\":\"app\"}\n"
                        .to_vec(),
                )]),
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

    // The wait holds the status request back; it is buffered, not lost.
    assert_eq!(first.turn.frames.len(), 1);
    assert!(matches!(
        first.turn.frames[0].frame,
        SupervisorControlFrameTurn::CommandAccepted { wait: Some(_), .. }
    ));
    let record = connections.get(44).expect("connection");
    assert!(record.state().pending_wait().is_some());
    assert_eq!(
        record.state().read_buffer().len(),
        b"{\"command\":\"status\",\"service\":\"app\"}\n".len(),
    );
    assert!(connections.fds_with_runnable_frames().is_empty());
    assert_eq!(connections.next_idle_deadline_ns(5), None);

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
        .flush_terminal_control_waits(&mut connections, 124, 0)
        .expect("flush waits");
    assert_eq!(flush.completed.len(), 1);

    // The wait has cleared, so the buffered request is runnable, and it keeps
    // the connection out of the idle count until it has been answered.
    assert_eq!(connections.fds_with_runnable_frames(), vec![44]);
    assert_eq!(connections.next_idle_deadline_ns(5), None);

    let resumed = supervisor
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
                observed_at_ns: 125,
            },
        )
        .expect("resumed connection turn");

    // Nothing new to read: the frame the turn answers was already buffered.
    assert!(matches!(
        resumed.turn.read,
        ControlConnectionReadTurn::WouldBlock { buffered_bytes } if buffered_bytes > 0
    ));
    assert_eq!(resumed.turn.frames.len(), 1);
    assert!(matches!(
        resumed.turn.frames[0].frame,
        SupervisorControlFrameTurn::CommandAccepted { wait: None, .. }
    ));
    let record = connections.get(44).expect("connection");
    assert!(record.state().read_buffer().is_empty());
    assert_eq!(
        record.state().idle_deadline_ns(5),
        Some(125 + 5_000_000_000)
    );
    let writes = record.io().writes.borrow();
    assert_eq!(writes.len(), 2);
    let wait_answer = response_json(&writes[0]);
    assert_eq!(wait_answer["status"], "ok");
    assert!(wait_answer["operation_id"].is_string());
    let status = response_json(&writes[1]);
    assert_eq!(status["status"], "ok");
    assert_eq!(status["service"], "app");
    assert_eq!(status["state"], "active");
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

    let frame = turn.turn.frames.into_iter().next().expect("frame").frame;
    let SupervisorControlFrameTurn::CommandAccepted {
        response_line: None,
        wait: Some(wait),
        ..
    } = frame
    else {
        panic!("expected registered wait");
    };

    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, LIFECYCLE_COMMAND_NS + 1_000_000_000, 0)
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
        json["message"].as_str().expect("message").contains(
            &wait
                .operation()
                .expect("operation wait")
                .operation_id
                .to_canonical_string()
        )
    );
}

/// A `wait=true` start is answered against the deadline the service is
/// actually held to. Before the fix the waiter used the definition's
/// StartTimeout from the request, so a service granted an extension was
/// still reported OPERATION_TIMEOUT at the original deadline, and then
/// succeeded (PEI-838).
#[test]
fn readiness_extension_moves_the_waiters_operation_timeout() {
    let mut app = inactive_alive_service("app");
    app.readiness = Readiness::Notify;
    app.start_timeout_secs = 10;
    let mut supervisor = booted_supervisor(vec![app]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new(
        (0..16)
            .map(|offset| LIFECYCLE_COMMAND_NS + offset)
            .collect::<Vec<_>>(),
    );
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            44,
            ControlConnectionRecord::new(
                FakeConnectionIo::scripted_reads([ControlSocketRead::Bytes(
                    b"{\"command\":\"start\",\"service\":\"app\",\"wait\":true}\n".to_vec(),
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
                observed_at_ns: LIFECYCLE_COMMAND_NS,
            },
        )
        .expect("connection turn");
    let SupervisorControlFrameTurn::CommandAccepted {
        wait: Some(wait), ..
    } = turn.turn.frames.into_iter().next().expect("frame").frame
    else {
        panic!("expected registered wait");
    };
    let operation_id = wait.operation().expect("operation wait").operation_id;

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
    let readiness = supervisor
        .next_readiness_timeout()
        .expect("readiness deadline");
    assert_eq!(readiness.operation_id, operation_id);

    // The service asks for more time, well inside the 4x cap.
    supervisor
        .apply_notify_datagram(
            NotifyDatagram {
                payload: b"EXTEND_TIMEOUT_USEC=5000000".to_vec(),
                credentials: NotifyCredentials {
                    pid: 9000,
                    uid: 0,
                    gid: 0,
                },
                fds: Vec::new(),
            },
            readiness.due_at_ns - 1_000_000_000,
            &mut controller,
        )
        .expect("extend readiness");
    let extended = supervisor
        .next_readiness_timeout()
        .expect("extended readiness deadline");
    assert_eq!(extended.due_at_ns, readiness.due_at_ns + 4_000_000_000);

    // At the original deadline the service is still inside its extension, so
    // the caller keeps waiting.
    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, readiness.due_at_ns, 0)
        .expect("flush at the original deadline");
    assert!(flush.completed.is_empty());
    assert!(
        connections
            .get(44)
            .expect("connection")
            .state()
            .pending_wait()
            .is_some()
    );

    // At the extended deadline the service has failed it, and so has the wait.
    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, extended.due_at_ns, 0)
        .expect("flush at the extended deadline");
    assert_eq!(flush.completed.len(), 1);
    let record = connections.get(44).expect("connection");
    assert!(record.state().pending_wait().is_none());
    let writes = record.io().writes.borrow();
    assert_eq!(writes.len(), 1);
    let json = response_json(&writes[0]);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "OPERATION_TIMEOUT");
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
        .set_pending_wait(ControlPendingWait::Operation(ControlOperationWait {
            operation_id,
            service: "app".to_string(),
        }));

    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, LIFECYCLE_COMMAND_NS, 0)
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

fn turn_context<'a>(
    access: &'a mut TestAccessChecker,
    controller: &'a mut TestProcessController,
    clock: &'a mut ScriptedClock,
    observed_at_ns: u64,
) -> SupervisorControlConnectionTurnContext<
    'a,
    'a,
    ScriptedClock,
    TestProcessController,
    TestAccessChecker,
> {
    SupervisorControlConnectionTurnContext {
        control_security: &DEFAULT_CONTROL_SECURITY,
        access_checker: access,
        controller,
        clock,
        registry: None,
        max_read_bytes: 1024,
        max_request_bytes: crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
        observed_at_ns,
    }
}

/// An Active `app` launched at boot, with a control connection holding a
/// `stop app` wait against it and the stop's SIGTERM already sent.
fn active_app_with_a_waiting_stop(
    clock: &mut ScriptedClock,
    access: &mut TestAccessChecker,
    controller: &mut TestProcessController,
    withdraw_definition: bool,
) -> (
    crate::supervisor::Supervisor,
    ControlConnectionTable<ControlConnectionRecord<FakeConnectionIo>>,
    crate::ids::JobId,
) {
    let mut supervisor = booted_supervisor(vec![crate::supervisor::tests::alive_service("app")]);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9000, 90)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, clock)
        .expect("launch app")
        .expect("app launch dispatch");
    let job = supervisor
        .service_status("app")
        .expect("active app")
        .current_job
        .expect("app job")
        .id;
    if withdraw_definition {
        supervisor
            .services
            .apply_definition_snapshot(Vec::new())
            .expect("withdraw the definition");
        assert!(
            supervisor
                .service_status("app")
                .expect("still supervised")
                .definition_removed
        );
    }

    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            44,
            ControlConnectionRecord::new(
                FakeConnectionIo::scripted_reads([ControlSocketRead::Bytes(
                    b"{\"command\":\"stop\",\"service\":\"app\"}\n".to_vec(),
                )]),
                control_peer(),
            ),
        )
        .expect("admit connection");
    let turn = supervisor
        .process_control_connection_table_turn(
            &mut connections,
            44,
            turn_context(access, controller, clock, 123),
        )
        .expect("connection turn");
    assert_eq!(turn.turn.frames.len(), 1);
    assert!(matches!(
        turn.turn.frames[0].frame,
        SupervisorControlFrameTurn::CommandAccepted {
            response_line: None,
            wait: Some(_),
            ..
        }
    ));
    (supervisor, connections, job)
}

fn drain(
    supervisor: &mut crate::supervisor::Supervisor,
    clock: &mut ScriptedClock,
    controller: &mut TestProcessController,
) {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(Vec::new());
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default();
    drain_runtime_work_queues(
        supervisor,
        &mut RuntimeWorkPumpContext {
            clock,
            controller,
            token_provider: &mut tokens,
            process_launcher: &mut launcher,
            filesystem_check_launcher: &mut filesystem_check_launcher,
            config: RuntimeWorkPumpConfig::default(),
        },
    )
    .expect("drain work");
}

/// PEI-803. `stop` is the one lifecycle command a definition-removed service
/// accepts (§3.8), and the stopped instance's exit discards the entry. The
/// wait's answer then looked the service up, found nothing, and the query
/// error failed the runtime loop: the client saw its connection dropped
/// mid-answer and PID 1 unlinked both sockets.
#[test]
fn stop_wait_on_a_definition_removed_service_is_answered_after_the_discard() {
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 1,
        LIFECYCLE_COMMAND_NS + 2,
        LIFECYCLE_COMMAND_NS + 3,
        LIFECYCLE_COMMAND_NS + 4,
    ]);
    let (mut supervisor, mut connections, job) =
        active_app_with_a_waiting_stop(&mut clock, &mut access, &mut controller, true);
    drain(&mut supervisor, &mut clock, &mut controller);
    assert_eq!(controller.signals.len(), 1, "the stop was executed");
    let wait = connections
        .get(44)
        .expect("connection")
        .state()
        .pending_wait()
        .and_then(|wait| wait.operation().cloned())
        .expect("operation wait");

    supervisor
        .complete_job(job, LIFECYCLE_COMMAND_NS + 10, 0)
        .expect("the stopped instance exits");
    assert!(
        supervisor.service_status("app").is_err(),
        "the entry took the ordinary removal discard"
    );

    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, LIFECYCLE_COMMAND_NS + 11, 0)
        .expect("a discarded entry does not fail the flush");

    assert_eq!(flush.completed.len(), 1);
    let record = connections.get(44).expect("connection");
    assert!(record.state().pending_wait().is_none());
    let writes = record.io().writes.borrow();
    assert_eq!(writes.len(), 1);
    let json = response_json(&writes[0]);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "app");
    assert_eq!(json["state"], "inactive");
    assert_eq!(json["cause"], "explicit_stop");
    assert_eq!(
        json["operation_id"],
        wait.operation_id.to_canonical_string()
    );
}

/// PEI-803. A waited operation that failed because peinit could not execute
/// it is answered with INTERNAL_ERROR, not an "ok" carrying an unchanged
/// state: the service did nothing wrong, and the client should not be told
/// its command succeeded.
#[test]
fn wait_on_an_operation_that_failed_before_it_began_is_answered_internal_error() {
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 1,
        LIFECYCLE_COMMAND_NS + 2,
        LIFECYCLE_COMMAND_NS + 3,
        LIFECYCLE_COMMAND_NS + 4,
    ]);
    let (mut supervisor, mut connections, job) =
        active_app_with_a_waiting_stop(&mut clock, &mut access, &mut controller, false);
    // The job finishes under the pending stop, bypassing the supervisor, so
    // the boundary finds no current main job to act on.
    supervisor
        .jobs_mut()
        .complete_job(job, LIFECYCLE_COMMAND_NS + 1, 0)
        .expect("finish the job under the pending stop");
    drain(&mut supervisor, &mut clock, &mut controller);
    assert!(controller.signals.is_empty(), "nothing was signalled");

    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, LIFECYCLE_COMMAND_NS + 11, 0)
        .expect("flush waits");

    assert_eq!(flush.completed.len(), 1);
    let record = connections.get(44).expect("connection");
    let writes = record.io().writes.borrow();
    assert_eq!(writes.len(), 1);
    let json = response_json(&writes[0]);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "INTERNAL_ERROR");
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("MissingCurrentMainJob")),
        "{json}"
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        crate::service::runtime::ServiceState::Active,
        "the service kept its state"
    );
}
