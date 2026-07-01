use crate::boundary::LinuxTimerFdRead;
use crate::control::connection::ControlConnectionTable;
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeShutdownEventSources,
    RuntimeShutdownEventTurn, RuntimeShutdownLoopContext, RuntimeWorkPumpConfig,
    process_runtime_shutdown_loop_turn,
};
use crate::service::runtime::ServiceState;
use crate::supervisor::{
    SupervisorLifecycleDeadlineKind, SupervisorLifecycleDeadlineTimerTurn,
    SupervisorWatchdogTimeoutOutcome,
};

use super::support::{
    AllowAccessChecker, DeadlineTimerCall, FakeBootAttemptCounter, FakeChildReaper,
    FakeControlListener, FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource,
    RuntimeFinalizer,
};
use crate::supervisor::tests::{
    APP_CRASH_NS, ScriptedClock, TestProcessController, TestProcessLauncher, TestTokenProvider,
};

pub(super) mod support;

use support::{
    FakeWaiter, active_app_supervisor, active_app_supervisor_with_reload_command,
    active_watchdog_app_supervisor, execute_and_launch_reload_command,
    execute_next_control_operation, notify_app_supervisor, pre_start_hook_supervisor,
    process_expired_lifecycle_deadline, reload_app, stop_app,
};

static DEFAULT_CONTROL_SECURITY: crate::control::system::ControlSecurityDescriptor =
    crate::control::system::ControlSecurityDescriptor::Default;

#[test]
fn runtime_loop_arms_lifecycle_timer_before_waiting() {
    let mut supervisor = notify_app_supervisor();
    let expected_deadline = supervisor
        .next_readiness_timeout()
        .expect("readiness timeout")
        .due_at_ns;
    let mut waiter = FakeWaiter::new(Vec::<RuntimeEventSource>::new());
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    let mut shutdown_timer = FakeDeadlineTimer::would_block();
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
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(Vec::new());
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_loop_turn(
        &mut supervisor,
        &mut waiter,
        &mut RuntimeShutdownEventSources {
            signal_source: &mut signal,
            child_reaper: &mut child_reaper,
            notify_source: &mut notify,
            control_listener: &mut listener,
            control_connections: &mut connections,
            deadline_timer: &mut shutdown_timer,
            lifecycle_timer: &mut lifecycle_timer,
            power_button_source: &mut power_button,
            filesystem_check_reader: &mut filesystem_check_reader,
            log_pipes: &mut log_pipes,
        },
        RuntimeShutdownLoopContext {
            clock: &mut clock,
            controller: &mut controller,
            finalizer: &mut finalizer,
            access_checker: &mut access,
            registrar: &mut registrar,
            token_provider: &mut tokens,
            process_launcher: &mut launcher,
            filesystem_check_launcher: &mut filesystem_check_launcher,
            boot_attempt_counter: &mut boot_attempt_counter,
            control_security: &DEFAULT_CONTROL_SECURITY,
            max_events: 8,
            control_limits: RuntimeControlLimits::new(
                1024,
                crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
                crate::control::socket::DEFAULT_CONNECTION_TIMEOUT_SECS,
            ),
            work_pump: RuntimeWorkPumpConfig::default(),
        },
    )
    .expect("loop turn");

    assert!(turn.pre_work.is_empty());
    assert!(turn.post_work.is_empty());
    assert!(turn.sources.is_empty());
    assert!(turn.turns.is_empty());
    assert_eq!(waiter.max_events, vec![8]);
    assert_eq!(
        lifecycle_timer.calls,
        vec![DeadlineTimerCall::Arm(expected_deadline)],
    );
}

#[test]
fn runtime_lifecycle_deadline_timer_drives_readiness_timeout_and_disarms() {
    let mut supervisor = notify_app_supervisor();
    let deadline = supervisor
        .next_readiness_timeout()
        .expect("readiness timeout");

    let (turn, lifecycle_timer, controller) =
        process_expired_lifecycle_deadline(&mut supervisor, deadline.due_at_ns);

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        read,
        drive: Some(drive),
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn::Armed { deadline: armed },
    } = turn
    else {
        panic!("expected lifecycle deadline turn: {turn:?}");
    };
    assert_eq!(read, LinuxTimerFdRead::Expired { expirations: 1 });
    assert_eq!(drive.readiness_timeouts.len(), 1);
    assert!(drive.pre_start_hook_timeouts.is_empty());
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Backoff,
    );
    assert!(matches!(
        &armed.kind,
        SupervisorLifecycleDeadlineKind::RestartBackoff { service } if service == "app"
    ));
    assert_eq!(
        lifecycle_timer.calls,
        vec![
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(armed.due_at_ns)
        ],
    );
}

#[test]
fn runtime_lifecycle_deadline_timer_drives_pre_start_hook_timeout() {
    let (mut supervisor, hook_job) = pre_start_hook_supervisor();
    let deadline = supervisor
        .next_pre_start_hook_timeout()
        .expect("pre-start hook timeout");

    let (turn, lifecycle_timer, controller) =
        process_expired_lifecycle_deadline(&mut supervisor, deadline.due_at_ns);

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive),
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn::Armed { deadline: armed },
        ..
    } = turn
    else {
        panic!("expected lifecycle deadline turn: {turn:?}");
    };
    assert_eq!(drive.pre_start_hook_timeouts.len(), 1);
    assert_eq!(
        drive.pre_start_hook_timeouts[0].timeout.job_event.job_id,
        hook_job,
    );
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Backoff,
    );
    assert!(matches!(
        &armed.kind,
        SupervisorLifecycleDeadlineKind::RestartBackoff { service } if service == "app"
    ));
    assert_eq!(
        lifecycle_timer.calls,
        vec![
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(armed.due_at_ns),
        ],
    );
}

#[test]
fn runtime_lifecycle_deadline_timer_drives_normal_stop_timeout() {
    let mut supervisor = active_app_supervisor();
    stop_app(&mut supervisor);
    execute_next_control_operation(&mut supervisor);
    let deadline = supervisor
        .next_stop_timeout_deadline()
        .expect("stop timeout");

    let (turn, lifecycle_timer, controller) =
        process_expired_lifecycle_deadline(&mut supervisor, deadline.due_at_ns);

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive),
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn::Armed { deadline: armed },
        ..
    } = turn
    else {
        panic!("expected lifecycle deadline turn: {turn:?}");
    };
    assert_eq!(drive.stop_timeouts.len(), 1);
    assert_eq!(drive.stop_timeouts[0].escalation.service, "app");
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/main".to_string()],
    );
    assert!(supervisor.next_stop_timeout_deadline().is_none());
    assert!(matches!(
        &armed.kind,
        SupervisorLifecycleDeadlineKind::CgroupCleanup { service, cgroup_id }
            if service == "app" && cgroup_id == "/sys/fs/cgroup/peinit/app/main"
    ));
    assert_eq!(
        lifecycle_timer.calls,
        vec![
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(armed.due_at_ns),
        ],
    );
}

#[test]
fn runtime_lifecycle_deadline_timer_drives_reload_detection_window() {
    let mut supervisor = active_app_supervisor();
    reload_app(&mut supervisor);
    execute_next_control_operation(&mut supervisor);
    let deadline = supervisor
        .next_reload_detection_deadline()
        .expect("reload detection");

    let (turn, lifecycle_timer, controller) =
        process_expired_lifecycle_deadline(&mut supervisor, deadline.due_at_ns);

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive),
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn::Armed { deadline: armed },
        ..
    } = turn
    else {
        panic!("expected lifecycle deadline turn: {turn:?}");
    };
    assert_eq!(drive.reload_detections.len(), 1);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert!(controller.cgroup_kills.is_empty());
    assert_eq!(armed.kind, SupervisorLifecycleDeadlineKind::BootSuccess);
    assert_eq!(
        lifecycle_timer.calls,
        vec![
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(armed.due_at_ns),
        ],
    );
}

#[test]
fn runtime_lifecycle_deadline_timer_drives_reload_command_timeout() {
    let mut supervisor = active_app_supervisor_with_reload_command();
    reload_app(&mut supervisor);
    let job_id = execute_and_launch_reload_command(&mut supervisor);
    let deadline = supervisor
        .next_reload_command_timeout()
        .expect("reload command timeout");

    let (turn, lifecycle_timer, controller) =
        process_expired_lifecycle_deadline(&mut supervisor, deadline.due_at_ns);

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive),
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn::Armed { deadline: armed },
        ..
    } = turn
    else {
        panic!("expected lifecycle deadline turn");
    };
    assert_eq!(drive.reload_command_timeouts.len(), 1);
    assert_eq!(
        drive.reload_command_timeouts[0].timeout.job_event.job_id,
        job_id,
    );
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks".to_string()],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert!(matches!(
        &armed.kind,
        SupervisorLifecycleDeadlineKind::CgroupCleanup { service, cgroup_id }
            if service == "app" && cgroup_id == "/sys/fs/cgroup/peinit/app/hooks"
    ));
    assert_eq!(
        lifecycle_timer.calls,
        vec![
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(armed.due_at_ns),
        ],
    );
}

#[test]
fn runtime_lifecycle_deadline_timer_drives_watchdog_timeout() {
    let mut supervisor = active_watchdog_app_supervisor();
    let deadline = supervisor
        .next_watchdog_timeout()
        .expect("watchdog timeout");

    let (turn, lifecycle_timer, controller) =
        process_expired_lifecycle_deadline(&mut supervisor, deadline.due_at_ns);

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive),
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn::Armed { deadline: armed },
        ..
    } = turn
    else {
        panic!("expected lifecycle deadline turn");
    };
    assert_eq!(drive.watchdog_timeouts.len(), 1);
    assert_eq!(
        drive.watchdog_timeouts[0].outcome,
        SupervisorWatchdogTimeoutOutcome::RestartScheduled,
    );
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Backoff,
    );
    assert!(matches!(
        &armed.kind,
        SupervisorLifecycleDeadlineKind::RestartBackoff { service } if service == "app"
    ));
    assert_eq!(
        lifecycle_timer.calls,
        vec![
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(armed.due_at_ns),
        ],
    );
}

#[test]
fn runtime_lifecycle_deadline_timer_drives_restart_backoff_and_arms_next_deadline() {
    let mut supervisor = active_app_supervisor();
    let job_id = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("app job")
        .id;
    supervisor
        .complete_job(job_id, APP_CRASH_NS, 1)
        .expect("crash app");
    let deadline = supervisor
        .next_restart_backoff_deadline()
        .expect("restart backoff");

    let (turn, lifecycle_timer, controller) =
        process_expired_lifecycle_deadline(&mut supervisor, deadline.due_at_ns);

    let RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        drive: Some(drive),
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn::Armed { deadline: armed },
        ..
    } = turn
    else {
        panic!("expected lifecycle deadline turn: {turn:?}");
    };
    assert_eq!(drive.restart_backoffs.len(), 1);
    assert_eq!(drive.restart_backoffs[0].due.service, "app");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
    assert!(controller.cgroup_kills.is_empty());
    assert_eq!(armed.kind, SupervisorLifecycleDeadlineKind::BootSuccess);
    assert_eq!(
        lifecycle_timer.calls,
        vec![
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(armed.due_at_ns),
        ],
    );
}
