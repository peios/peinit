use crate::control::connection::{ControlConnectionRecord, ControlConnectionTable};
use crate::control::socket::{ControlSocketRead, ControlSocketWrite};
use crate::runtime::{
    RuntimeEventSource, RuntimeShutdownEventSources, RuntimeShutdownEventTurn,
    process_runtime_shutdown_event,
};
use crate::shutdown::ShutdownKind;
use crate::supervisor::{SupervisorControlCommandDispatch, SupervisorControlFrameTurn};

use super::super::super::SHUTDOWN_NS;
use super::super::super::fixture::{DRAINING_STOP_DEADLINE_NS, shutdown_fixture};
use super::super::support::{
    AllowAccessChecker, DeadlineTimerCall, FakeAcceptedConnection, FakeBootAttemptCounter,
    FakeChildReaper, FakeControlListener, FakeDeadlineTimer, FakeNotifySource, FakeRegistrar,
    FakeSignalSource, RuntimeFinalizer, context, control_peer,
};
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn runtime_control_connection_event_accepts_shutdown_request_and_arms_deadline_timer() {
    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            46,
            ControlConnectionRecord::new(
                FakeAcceptedConnection::with_io(
                    46,
                    "admin",
                    [ControlSocketRead::Bytes(
                        b"{\"command\":\"shutdown\",\"type\":\"reboot\"}\n".to_vec(),
                    )],
                    [ControlSocketWrite::Complete],
                ),
                control_peer("admin"),
            ),
        )
        .expect("seed connection");
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([123, SHUTDOWN_NS]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let mut jobs_channel = crate::runtime::NoJobsChannel;
    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::ControlConnection { fd: 46 },
        &mut RuntimeShutdownEventSources {
            signal_source: &mut signal,
            child_reaper: &mut child_reaper,
            notify_source: &mut notify,
            control_listener: &mut listener,
            control_connections: &mut connections,
            deadline_timer: &mut deadline_timer,
            lifecycle_timer: &mut lifecycle_timer,
            power_button_source: &mut power_button,
            filesystem_check_reader: &mut filesystem_check_reader,
            log_pipes: &mut log_pipes,
            jobs_channel: &mut jobs_channel,
        },
        context(
            &mut clock,
            &mut controller,
            &mut finalizer,
            &mut access,
            &mut registrar,
            &mut boot_attempt_counter,
        ),
    )
    .expect("runtime event");

    let RuntimeShutdownEventTurn::ControlConnection {
        supervisor: connection_turn,
        deadline_timer: Some(_),
        ..
    } = turn
    else {
        panic!("expected control connection turn");
    };
    assert!(matches!(
        connection_turn.turn.frames.into_iter().next().expect("frame").frame,
        SupervisorControlFrameTurn::CommandAccepted {
            dispatch: Some(dispatch),
            ..
        } if matches!(&*dispatch, SupervisorControlCommandDispatch::Shutdown(_)),
    ));
    assert_eq!(
        deadline_timer.calls,
        vec![DeadlineTimerCall::Arm(DRAINING_STOP_DEADLINE_NS)],
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Reboot,
    );
}

#[test]
fn resuming_buffered_control_frames_answers_a_request_no_event_will_come_for() {
    let mut supervisor = shutdown_fixture();
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            46,
            ControlConnectionRecord::new(
                FakeAcceptedConnection::with_io(46, "admin", [], [ControlSocketWrite::Complete]),
                control_peer("admin"),
            ),
        )
        .expect("seed connection");
    // A request that arrived in the same read as a wait, and sat behind it:
    // already read, so the socket will never report it readable again.
    connections
        .get_mut(46)
        .expect("connection")
        .state_mut()
        .read_buffer_mut()
        .append(b"{\"command\":\"list\"}\n");
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut clock = ScriptedClock::new([123]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turns = crate::runtime::resume_buffered_control_frames(
        &mut supervisor,
        &mut connections,
        None::<&mut crate::runtime::NoRuntimeRegistryClient>,
        &mut deadline_timer,
        context(
            &mut clock,
            &mut controller,
            &mut finalizer,
            &mut access,
            &mut registrar,
            &mut boot_attempt_counter,
        ),
    )
    .expect("resume buffered frames");

    assert_eq!(turns.len(), 1);
    let RuntimeShutdownEventTurn::ControlConnection {
        fd: 46,
        supervisor: connection_turn,
        deadline_timer: None,
    } = &turns[0]
    else {
        panic!("expected a control connection turn for the buffered request");
    };
    assert_eq!(connection_turn.turn.frames.len(), 1);
    assert!(matches!(
        connection_turn.turn.frames[0].frame,
        SupervisorControlFrameTurn::CommandAccepted {
            response_line: Some(_),
            ..
        }
    ));
    assert!(matches!(
        connection_turn.turn.write,
        crate::control::connection::ControlConnectionWriteTurn::Complete { written, .. } if written > 0
    ));
    let record = connections.get(46).expect("connection");
    assert!(record.state().read_buffer().is_empty());
    assert!(connections.fds_with_runnable_frames().is_empty());
}

#[test]
fn runtime_control_connection_event_ignores_stale_removed_fd() {
    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let mut jobs_channel = crate::runtime::NoJobsChannel;
    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::ControlConnection { fd: 99 },
        &mut RuntimeShutdownEventSources {
            signal_source: &mut signal,
            child_reaper: &mut child_reaper,
            notify_source: &mut notify,
            control_listener: &mut listener,
            control_connections: &mut connections,
            deadline_timer: &mut deadline_timer,
            lifecycle_timer: &mut lifecycle_timer,
            power_button_source: &mut power_button,
            filesystem_check_reader: &mut filesystem_check_reader,
            log_pipes: &mut log_pipes,
            jobs_channel: &mut jobs_channel,
        },
        context(
            &mut clock,
            &mut controller,
            &mut finalizer,
            &mut access,
            &mut registrar,
            &mut boot_attempt_counter,
        ),
    )
    .expect("runtime event");

    assert_eq!(
        turn,
        RuntimeShutdownEventTurn::StaleControlConnection { fd: 99 },
    );
    assert!(deadline_timer.calls.is_empty());
    assert!(connections.is_empty());
}
