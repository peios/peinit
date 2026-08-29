use crate::boundary::{LinuxPowerButtonRead, LinuxPowerButtonReadError};
use crate::runtime::{
    RuntimeEventSource, RuntimePowerButtonTurn, RuntimeShutdownEventSources,
    RuntimeShutdownEventTurn, process_runtime_shutdown_event,
};
use crate::shutdown::ShutdownKind;
use crate::supervisor::{SupervisorPowerButtonAction, SupervisorShutdownDeadlineTimerTurn};

use super::super::SHUTDOWN_NS;
use super::super::fixture::{DRAINING_STOP_DEADLINE_NS, shutdown_fixture};
use super::support::{
    AllowAccessChecker, DeadlineTimerCall, FakeBootAttemptCounter, FakeChildReaper,
    FakeControlListener, FakeDeadlineTimer, FakeNotifySource, FakePowerButtonSource, FakeRegistrar,
    FakeSignalSource, RuntimeFinalizer, context,
};
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn runtime_power_button_press_enters_poweroff_and_arms_deadline_timer() {
    let mut supervisor = shutdown_fixture();
    let mut power_button = FakePowerButtonSource::new([Ok(LinuxPowerButtonRead::Pressed)]);
    let mut sources = TestSources::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::PowerButton { fd: 17 },
        &mut sources.event_sources(&mut power_button),
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

    let RuntimeShutdownEventTurn::PowerButton {
        turn:
            RuntimePowerButtonTurn::Shutdown {
                read,
                supervisor: dispatch,
                deadline_timer: Some(SupervisorShutdownDeadlineTimerTurn::Armed { deadline }),
            },
        ..
    } = turn
    else {
        panic!("expected power-button shutdown turn");
    };
    assert_eq!(read, LinuxPowerButtonRead::Pressed);
    assert_eq!(deadline.due_at_ns, DRAINING_STOP_DEADLINE_NS);
    assert!(matches!(
        dispatch.action,
        SupervisorPowerButtonAction::Graceful(_),
    ));
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Poweroff,
    );
    assert_eq!(
        sources.deadline_timer.calls,
        vec![DeadlineTimerCall::Arm(DRAINING_STOP_DEADLINE_NS)],
    );
}

#[test]
fn runtime_power_button_non_press_event_is_ignored() {
    let mut supervisor = shutdown_fixture();
    let mut power_button = FakePowerButtonSource::new([Ok(LinuxPowerButtonRead::Ignored)]);
    let mut sources = TestSources::default();
    let mut clock = ScriptedClock::new([]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::PowerButton { fd: 17 },
        &mut sources.event_sources(&mut power_button),
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
        RuntimeShutdownEventTurn::PowerButton {
            turn: RuntimePowerButtonTurn::Ignored {
                read: LinuxPowerButtonRead::Ignored
            },
            ..
        }
    ));
    assert!(supervisor.shutdown().is_none());
    assert!(sources.deadline_timer.calls.is_empty());
}

#[test]
fn runtime_power_button_read_failure_disables_source_without_failing_turn() {
    let mut supervisor = shutdown_fixture();
    let mut power_button = FakePowerButtonSource::new([Err(LinuxPowerButtonReadError::Read {
        fd: 0,
        message: "input read failed".to_string(),
    })]);
    let mut sources = TestSources::default();
    let mut clock = ScriptedClock::new([]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::PowerButton { fd: 17 },
        &mut sources.event_sources(&mut power_button),
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
        RuntimeShutdownEventTurn::PowerButton {
            turn: RuntimePowerButtonTurn::ReadFailed {
                fd: 17,
                source_disabled: true,
                ..
            },
            ..
        }
    ));
    assert_eq!(registrar.unregister_calls, vec![17]);
    assert!(supervisor.shutdown().is_none());
}

struct TestSources {
    signal: FakeSignalSource,
    child_reaper: FakeChildReaper,
    notify: FakeNotifySource,
    listener: FakeControlListener,
    connections: crate::control::connection::ControlConnectionTable<
        crate::control::connection::ControlConnectionRecord<super::support::FakeAcceptedConnection>,
    >,
    deadline_timer: FakeDeadlineTimer,
    lifecycle_timer: FakeDeadlineTimer,
    filesystem_check_reader: crate::supervisor::tests::TestFilesystemCheckReader,
    log_pipes: crate::runtime::RuntimeServiceLogPipes,
    jobs_channel: crate::runtime::NoJobsChannel,
}

impl TestSources {
    fn event_sources<'a>(
        &'a mut self,
        power_button: &'a mut FakePowerButtonSource,
    ) -> RuntimeShutdownEventSources<
        'a,
        super::support::FakeAcceptedConnection,
        FakeControlListener,
        FakeSignalSource,
        FakeChildReaper,
        FakeNotifySource,
        FakeDeadlineTimer,
        FakeDeadlineTimer,
    > {
        RuntimeShutdownEventSources {
            signal_source: &mut self.signal,
            child_reaper: &mut self.child_reaper,
            notify_source: &mut self.notify,
            control_listener: &mut self.listener,
            control_connections: &mut self.connections,
            deadline_timer: &mut self.deadline_timer,
            lifecycle_timer: &mut self.lifecycle_timer,
            power_button_source: power_button,
            filesystem_check_reader: &mut self.filesystem_check_reader,
            log_pipes: &mut self.log_pipes,
            jobs_channel: &mut self.jobs_channel,
        }
    }
}

impl Default for TestSources {
    fn default() -> Self {
        Self {
            signal: FakeSignalSource::would_block(),
            child_reaper: FakeChildReaper::empty(),
            notify: FakeNotifySource::empty(),
            listener: FakeControlListener::default(),
            connections: crate::control::connection::ControlConnectionTable::new(4),
            deadline_timer: FakeDeadlineTimer::would_block(),
            lifecycle_timer: FakeDeadlineTimer::would_block(),
            filesystem_check_reader: crate::supervisor::tests::TestFilesystemCheckReader::default(),
            log_pipes: crate::runtime::RuntimeServiceLogPipes::default(),
            jobs_channel: crate::runtime::NoJobsChannel,
        }
    }
}
