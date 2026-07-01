use crate::runtime::{
    RuntimeEventSource, RuntimeJfsDeviceTurn, RuntimeShutdownEventSources,
    RuntimeShutdownEventTurn, process_runtime_shutdown_event,
};
use crate::supervisor::Supervisor;

use super::support::{
    AllowAccessChecker, FakeBootAttemptCounter, FakeChildReaper, FakeControlListener,
    FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource, RuntimeFinalizer,
    context,
};
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn runtime_jfs_event_reaches_parse_boundary_and_disables_source_until_abi_exists() {
    let mut supervisor = Supervisor::default();
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::JfsDevice { fd: 55 },
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
        RuntimeShutdownEventTurn::JfsDevice {
            turn: RuntimeJfsDeviceTurn::ParseBoundaryReached {
                fd: 55,
                source_disabled_until_abi_exists: true,
            },
        },
    );
    assert_eq!(registrar.unregister_calls, vec![55]);
}
