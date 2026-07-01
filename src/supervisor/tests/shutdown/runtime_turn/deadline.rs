use crate::boundary::LinuxTimerFdRead;
use crate::runtime::{
    RuntimeEventSource, RuntimeShutdownEventSources, RuntimeShutdownEventTurn,
    process_runtime_shutdown_event,
};
use crate::shutdown::{ShutdownDeadlineKind, ShutdownKind};

use super::super::SHUTDOWN_NS;
use super::super::fixture::{DRAINING_STOP_DEADLINE_NS, drive_shutdown_to_ready, shutdown_fixture};
use super::support::{
    AllowAccessChecker, DeadlineTimerCall, FakeBootAttemptCounter, FakeChildReaper,
    FakeControlListener, FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource,
    RuntimeFinalizer, context,
};
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

const POST_KILL_TIMEOUT_NS: u64 = 5_000_000_000;

#[test]
fn runtime_shutdown_deadline_timer_event_drives_due_shutdown_work_and_rearms() {
    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::expired_once();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([DRAINING_STOP_DEADLINE_NS]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    controller.cgroup_kills.clear();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::ShutdownDeadlineTimer,
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

    let RuntimeShutdownEventTurn::ShutdownDeadlineTimer {
        read,
        drive: Some(drive),
        deadline_timer: crate::supervisor::SupervisorShutdownDeadlineTimerTurn::Armed { deadline },
    } = turn
    else {
        panic!("expected deadline timer turn");
    };
    assert_eq!(read, LinuxTimerFdRead::Expired { expirations: 1 });
    assert_eq!(drive.timeout.expect("timeout").cgroup_kills.len(), 1);
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/draining".to_string()],
    );
    assert_eq!(
        deadline.kind,
        ShutdownDeadlineKind::PostKillTimeout {
            service: "draining".to_string(),
        },
    );
    assert_eq!(
        deadline_timer.calls,
        vec![
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(DRAINING_STOP_DEADLINE_NS + POST_KILL_TIMEOUT_NS),
        ],
    );
}

#[test]
fn runtime_deadline_timer_event_can_finalize_without_rebooting_host() {
    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::expired_once();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS + 100]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();
    drive_shutdown_to_ready(&mut supervisor, ShutdownKind::Poweroff, &mut controller);

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::ShutdownDeadlineTimer,
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

    assert!(matches!(
        turn,
        RuntimeShutdownEventTurn::ShutdownDeadlineTimer {
            drive: Some(_),
            deadline_timer: crate::supervisor::SupervisorShutdownDeadlineTimerTurn::Disarmed,
            ..
        },
    ));
    assert_eq!(
        finalizer.calls,
        vec![
            super::support::RuntimeFinalizerCall::Snapshot,
            super::support::RuntimeFinalizerCall::Remount("/".to_string()),
            super::support::RuntimeFinalizerCall::Sync,
            super::support::RuntimeFinalizerCall::Reboot(ShutdownKind::Poweroff),
        ],
    );
    assert_eq!(
        deadline_timer.calls,
        vec![DeadlineTimerCall::Read, DeadlineTimerCall::Disarm],
    );
}
