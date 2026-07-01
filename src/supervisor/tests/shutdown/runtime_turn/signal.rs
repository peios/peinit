use crate::boundary::{ChildExitStatus, ChildReap, LinuxSignalFdRead, ProcessSignal};
use crate::runtime::{
    RuntimeEventSource, RuntimeShutdownEventSources, RuntimeShutdownEventTurn,
    process_runtime_shutdown_event,
};
use crate::service::runtime::ServiceState;
use crate::shutdown::{
    ShutdownDeadlineKind, ShutdownFinalizationState, ShutdownKind, ShutdownSignal,
};
use crate::supervisor::{SupervisorChildReapDispatch, SupervisorChildReapTurn};

use super::super::SHUTDOWN_NS;
use super::super::fixture::{DRAINING_STOP_DEADLINE_NS, job_for, shutdown_fixture};
use super::support::{
    DeadlineTimerCall, FakeBootAttemptCounter, FakeChildReaper, FakeControlListener,
    FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource, RuntimeFinalizer,
    RuntimeFinalizerCall, context,
};
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

#[test]
fn runtime_pid1_signal_event_enters_shutdown_and_arms_deadline_timer() {
    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::new([LinuxSignalFdRead::Shutdown(ShutdownSignal::Sigterm)]);
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
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = super::support::AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::Pid1Signal,
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

    let RuntimeShutdownEventTurn::Pid1Signal {
        read,
        deadline_timer: Some(deadline),
        ..
    } = turn
    else {
        panic!("expected pid1 signal turn");
    };
    assert_eq!(read, LinuxSignalFdRead::Shutdown(ShutdownSignal::Sigterm));
    assert!(matches!(
        deadline,
        crate::supervisor::SupervisorShutdownDeadlineTimerTurn::Armed { .. },
    ));
    assert_eq!(
        deadline_timer.calls,
        vec![DeadlineTimerCall::Arm(DRAINING_STOP_DEADLINE_NS)],
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Poweroff,
    );
}

#[test]
fn runtime_sigchld_event_reaps_tracked_and_untracked_children() {
    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::new([LinuxSignalFdRead::Other {
        signal: libc::SIGCHLD,
    }]);
    let mut child_reaper = FakeChildReaper::new([Ok(vec![
        ChildReap {
            pid: 4242,
            status: ChildExitStatus::Exited { code: 0 },
        },
        ChildReap {
            pid: 8000,
            status: ChildExitStatus::Exited { code: 0 },
        },
    ])]);
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS + 1]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = super::support::AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::Pid1Signal,
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

    let RuntimeShutdownEventTurn::Pid1Signal {
        child_reaps,
        deadline_timer,
        ..
    } = turn
    else {
        panic!("expected pid1 signal turn");
    };
    assert!(deadline_timer.is_none());
    assert_eq!(child_reaper.calls, 1);
    assert!(matches!(
        child_reaps.as_slice(),
        [
            SupervisorChildReapTurn::Untracked {
                child: ChildReap { pid: 4242, .. },
            },
            SupervisorChildReapTurn::Tracked {
                child: ChildReap { pid: 8000, .. },
                dispatch: SupervisorChildReapDispatch::Runtime(_),
                ..
            },
        ],
    ));
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );
}

#[test]
fn runtime_sigchld_event_advances_shutdown_waves_and_resyncs_deadline_timer() {
    const REAP_NS: u64 = SHUTDOWN_NS + 1_000;
    const DB_STOP_DEADLINE_NS: u64 = REAP_NS + 10_000_000_000;

    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::new([LinuxSignalFdRead::Other {
        signal: libc::SIGCHLD,
    }]);
    let mut child_reaper = FakeChildReaper::new([Ok(vec![
        ChildReap {
            pid: 8000,
            status: ChildExitStatus::Exited { code: 0 },
        },
        ChildReap {
            pid: 8100,
            status: ChildExitStatus::Signaled {
                signal: libc::SIGTERM,
                core_dumped: false,
            },
        },
    ])]);
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([REAP_NS]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = super::support::AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    controller.signals.clear();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::Pid1Signal,
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

    let RuntimeShutdownEventTurn::Pid1Signal {
        child_reaps,
        deadline_timer:
            Some(crate::supervisor::SupervisorShutdownDeadlineTimerTurn::Armed { deadline }),
        ..
    } = turn
    else {
        panic!("expected shutdown child reaps and armed timer");
    };
    assert_eq!(child_reaper.calls, 1);
    assert_eq!(child_reaps.len(), 2);
    assert!(child_reaps.iter().all(|reap| matches!(
        reap,
        SupervisorChildReapTurn::Tracked {
            dispatch: SupervisorChildReapDispatch::Shutdown(_),
            ..
        }
    )));
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );
    assert_eq!(
        supervisor
            .service_status("draining")
            .expect("draining")
            .state,
        ServiceState::Inactive,
    );
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Stopping,
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").finalization,
        ShutdownFinalizationState::WaitingForServices,
    );
    assert_eq!(controller.signals.len(), 1);
    assert_eq!(controller.signals[0].target.service, "db");
    assert_eq!(controller.signals[0].signal, ProcessSignal::Sigterm);
    assert_eq!(deadline.due_at_ns, DB_STOP_DEADLINE_NS);
    assert_eq!(
        deadline.kind,
        ShutdownDeadlineKind::StopTimeout {
            service: "db".to_string(),
        },
    );
    assert_eq!(
        deadline_timer.calls,
        vec![DeadlineTimerCall::Arm(DB_STOP_DEADLINE_NS)],
    );
}

#[test]
fn runtime_sigchld_event_finalizes_shutdown_when_last_service_exits() {
    const APP_EXIT_NS: u64 = SHUTDOWN_NS + 1;
    const DRAINING_EXIT_NS: u64 = SHUTDOWN_NS + 2;
    const DB_REAP_NS: u64 = SHUTDOWN_NS + 3;

    let mut supervisor = shutdown_fixture();
    let mut signal = FakeSignalSource::new([LinuxSignalFdRead::Other {
        signal: libc::SIGCHLD,
    }]);
    let mut child_reaper = FakeChildReaper::new([Ok(vec![ChildReap {
        pid: 7000,
        status: ChildExitStatus::Exited { code: 0 },
    }])]);
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([DB_REAP_NS]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = super::support::AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();
    let app_job = job_for(&supervisor, "app");
    let draining_job = job_for(&supervisor, "draining");
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    supervisor
        .complete_shutdown_job(app_job, APP_EXIT_NS, 0, &mut controller)
        .expect("complete app");
    supervisor
        .complete_shutdown_job(draining_job, DRAINING_EXIT_NS, 0, &mut controller)
        .expect("complete draining");
    controller.signals.clear();

    let turn = process_runtime_shutdown_event(
        &mut supervisor,
        RuntimeEventSource::Pid1Signal,
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

    let RuntimeShutdownEventTurn::Pid1Signal {
        child_reaps,
        drive: Some(drive),
        deadline_timer: Some(crate::supervisor::SupervisorShutdownDeadlineTimerTurn::Disarmed),
        ..
    } = turn
    else {
        panic!("expected final child reap to drive shutdown finalization");
    };
    assert_eq!(child_reaper.calls, 1);
    assert!(matches!(
        child_reaps.as_slice(),
        [SupervisorChildReapTurn::Tracked {
            child: ChildReap { pid: 7000, .. },
            dispatch: SupervisorChildReapDispatch::Shutdown(_),
            ..
        }],
    ));
    assert!(drive.timeout.is_none());
    assert_eq!(
        drive.finalization.expect("finalization").finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(
        finalizer.calls,
        vec![
            RuntimeFinalizerCall::Snapshot,
            RuntimeFinalizerCall::Remount("/".to_string()),
            RuntimeFinalizerCall::Sync,
            RuntimeFinalizerCall::Reboot(ShutdownKind::Poweroff),
        ],
    );
    assert_eq!(deadline_timer.calls, vec![DeadlineTimerCall::Disarm]);
}
