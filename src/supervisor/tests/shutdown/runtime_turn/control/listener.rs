use crate::control::connection::ControlConnectionTable;
use crate::runtime::{
    RuntimeEventSource, RuntimeShutdownEventSources, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError, process_runtime_shutdown_event,
};

use super::super::support::{
    AllowAccessChecker, FakeAcceptedConnection, FakeBootAttemptCounter, FakeChildReaper,
    FakeControlListener, FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource,
    RegistrarCall, RuntimeFinalizer, context,
};
use crate::supervisor::tests::shutdown::fixture::shutdown_fixture;
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn runtime_control_listener_event_accepts_and_registers_connection_fd() {
    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::accepts([FakeAcceptedConnection::new(44, "admin")]);
    let mut connections = ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([123]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let mut jobs_channel = crate::runtime::NoJobsChannel;
    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::ControlListener,
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

    assert!(matches!(
        turn,
        RuntimeShutdownEventTurn::ControlListener {
            registration: Some(RuntimeEventSource::ControlConnection { fd: 44 }),
            ..
        },
    ));
    assert!(connections.get(44).is_some());
    assert_eq!(
        registrar.calls,
        vec![RegistrarCall {
            fd: 44,
            source: RuntimeEventSource::ControlConnection { fd: 44 },
        }],
    );
}

#[test]
fn runtime_control_listener_event_closes_accepted_connection_when_registration_fails() {
    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::accepts([FakeAcceptedConnection::new(45, "admin")]);
    let mut connections = ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([123]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::failing_registration();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let mut jobs_channel = crate::runtime::NoJobsChannel;
    let err = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::ControlListener,
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
    .expect_err("registration failure");

    assert!(matches!(
        err,
        RuntimeShutdownEventTurnError::ControlRegistration(_),
    ));
    assert!(connections.is_empty());
}
