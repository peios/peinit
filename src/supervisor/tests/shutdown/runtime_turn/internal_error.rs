//! PEI-1125: an internal error on a per-service path fails that service,
//! not supervision.
//!
//! Each test stages a fault peinit's own bookkeeping would not create, on a
//! path that is about one service — a job terminal, a health probe's
//! terminal, a launch's setup status, a control operation's execution — and
//! drives the runtime loop over it. The loop has to return a turn rather
//! than an error, the service has to be Failed under `InternalError`, and
//! the console has to say so. The last test is the other half of the rule:
//! an error about supervision itself still ends the loop.

use crate::boundary::{ChildExitStatus, ChildReap, LinuxSignalFdRead};
use crate::job::JobState;
use crate::operation::store::OperationRequest;
use crate::operation::{OperationSource, OperationState, OperationType, is_internal_error_result};
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeEventWaitError, RuntimeEventWaiter,
    RuntimeProcessSetupTurn, RuntimeShutdownEventSources, RuntimeShutdownEventTurn,
    RuntimeShutdownLoopContext, RuntimeShutdownLoopError, RuntimeShutdownLoopTurn,
    RuntimeWorkPumpConfig, process_runtime_shutdown_event, process_runtime_shutdown_loop_turn,
    process_runtime_shutdown_sources_with_registry,
};
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::tests::health::support::{
    active_health_supervisor, launch_due_health_check,
};
use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};
use crate::supervisor::{
    PendingControlOperation, PendingControlRequirement, Supervisor, SupervisorChildReapTurn,
    SupervisorSettings,
};

use super::super::SHUTDOWN_NS;
use super::super::fixture::{job_for, shutdown_fixture};
use super::support::{
    AllowAccessChecker, FakeBootAttemptCounter, FakeChildReaper, FakeControlListener,
    FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource, RuntimeFinalizer,
    context,
};

static DEFAULT_CONTROL_SECURITY: crate::control::system::ControlSecurityDescriptor =
    crate::control::system::ControlSecurityDescriptor::Default;

/// Descriptor numbers no test process has open, so the runtime's `close(2)`
/// of a retired setup's descriptors is a harmless EBADF rather than the
/// harness's own pipe.
const UNUSED_SETUP_FD: i32 = 1_000_055;
const UNUSED_PIDFD: i32 = 1_000_009;

/// The job-terminal class. `app`'s main job record is staged back to
/// `Created` while its pid is still indexed, so the exit's `complete_job`
/// is refused — a `JobStore` transition error out of the reap, which is
/// the shape PEI-824's rows took. Driven through the whole loop turn.
#[test]
fn an_internal_error_on_a_job_terminal_fails_the_service_and_the_loop_turn_returns() {
    let mut supervisor = shutdown_fixture();
    let app_job = job_for(&supervisor, "app");
    supervisor
        .jobs_mut()
        .record_mut(app_job)
        .expect("app job")
        .state = JobState::Created;
    let mut controller = TestProcessController::default();

    let turn = loop_turn(
        &mut supervisor,
        &mut controller,
        vec![RuntimeEventSource::Pid1Signal],
        Ok(vec![ChildReap {
            pid: 8000,
            status: ChildExitStatus::Exited { code: 0 },
        }]),
    )
    .expect("a per-service error is not a loop error");

    let RuntimeShutdownEventTurn::Pid1Signal { child_reaps, .. } = &turn.turns[0] else {
        panic!("expected the signal turn, got {:?}", turn.turns);
    };
    let [SupervisorChildReapTurn::InternalError { child, dispatch }] = child_reaps.as_slice()
    else {
        panic!("expected the reap to be contained, got {child_reaps:?}");
    };
    assert_eq!(child.pid, 8000);
    assert_eq!(dispatch.step, "job terminal");
    assert_eq!(dispatch.service(), Some("app"));
    assert_eq!(dispatch.subject.job_id, Some(app_job));
    assert!(dispatch.error.contains("Transition"), "{}", dispatch.error);
    // The service is failed under the cause that names peinit, not the
    // service; the job is retired; nothing is left to supervise.
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Failed);
    assert_eq!(status.cause, Some(TransitionCause::InternalError));
    assert!(supervisor.jobs().get(app_job).is_none());
    assert_eq!(
        dispatch.job_event.as_ref().map(|event| event.job_id),
        Some(app_job)
    );
    assert!(
        controller
            .cgroup_kills
            .iter()
            .any(|cgroup| cgroup.contains("app")),
        "the retired job's cgroup is killed: {:?}",
        controller.cgroup_kills,
    );
    let messages = console_lines(&turn);
    assert!(
        messages
            .iter()
            .any(|line| line.contains("service app: internal error at job terminal")),
        "{messages:?}",
    );
    assert!(
        messages
            .iter()
            .any(|line| line.contains("service app failed: InternalError")),
        "{messages:?}",
    );
    #[cfg(feature = "peios-boundary")]
    {
        let types = kmes_event_types(&turn);
        assert!(
            types.contains(&"service.internal_error".to_string()),
            "{types:?}"
        );
        assert!(types.contains(&"job.ended".to_string()), "{types:?}");
    }
}

/// The health-terminal class. The probe's cgroup kill is scripted to fail,
/// which reaches the loop as `SupervisorError::Health(Boundary)`. The
/// service is failed, both the probe and the main job are retired, and the
/// health deadlines that would have fired against a Failed service are
/// gone with them.
#[test]
fn an_internal_error_on_a_health_terminal_fails_the_service_not_the_loop() {
    let (mut supervisor, first_due) = active_health_supervisor(3);
    let launch = launch_due_health_check(&mut supervisor, first_due, 9000, UNUSED_PIDFD);
    let health_job = launch.launch.job_id;
    let main_job = supervisor
        .jobs()
        .current_service_main_job("app")
        .expect("main job");
    let mut controller = TestProcessController::default();
    controller.set_cgroup_kill_error(
        "/sys/fs/cgroup/peinit/app/health",
        "Operation not permitted (os error 1)",
    );
    let mut signal = FakeSignalSource::new([LinuxSignalFdRead::Other {
        signal: libc::SIGCHLD,
    }]);
    let mut child_reaper = FakeChildReaper::new([Ok(vec![ChildReap {
        pid: 9000,
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
    let mut clock = ScriptedClock::new([first_due + 200_000]);
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();
    let mut jobs_channel = crate::runtime::NoJobsChannel;

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
    .expect("a per-service error is not a runtime event error");

    let RuntimeShutdownEventTurn::Pid1Signal { child_reaps, .. } = &turn else {
        panic!("expected the signal turn, got {turn:?}");
    };
    let [SupervisorChildReapTurn::InternalError { dispatch, .. }] = child_reaps.as_slice() else {
        panic!("expected the reap to be contained, got {child_reaps:?}");
    };
    assert_eq!(dispatch.service(), Some("app"));
    assert_eq!(dispatch.subject.job_id, Some(health_job));
    assert!(
        dispatch.error.contains("Operation not permitted"),
        "{}",
        dispatch.error
    );
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Failed);
    assert_eq!(status.cause, Some(TransitionCause::InternalError));
    assert!(supervisor.jobs().get(health_job).is_none());
    assert!(
        supervisor.jobs().get(main_job).is_none(),
        "a Failed service does not keep a running main job",
    );
    assert_eq!(
        dispatch
            .service_job_event
            .as_ref()
            .map(|event| event.job_id),
        Some(main_job)
    );
    assert!(supervisor.next_health_check_timeout().is_none());
    assert!(supervisor.next_health_check_interval().is_none());
    assert!(supervisor.next_restart_backoff_deadline().is_none());
}

/// The launch-step class. `app` is launched into a pending setup and the
/// setup-status readiness arrives, but the status cannot be read — the test
/// launcher's reader fails for any descriptor, as `read(2)` on a descriptor
/// that has gone would (PEI-826's shape). The launching service is failed
/// from Starting, its boot operation fails with the `internal_error`
/// result, the job and its setup are gone, and the descriptor is out of
/// epoll.
#[test]
fn an_internal_error_on_a_setup_step_fails_the_launching_service_not_the_loop() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![alive_service("app")]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let job_id = supervisor.pending_launch_jobs()[0];
    let operation_id = supervisor
        .jobs()
        .get(job_id)
        .expect("job")
        .operation_id
        .expect("boot start operation");
    let mut pending = process(4242, UNUSED_PIDFD);
    pending.setup_status_fd = Some(UNUSED_SETUP_FD);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![pending]);
    let mut launch_clock = ScriptedClock::new([APP_LAUNCH_NS]);
    supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut launch_clock)
        .expect("launch app")
        .expect("pending launch dispatch");
    assert!(supervisor.has_pending_process_setup(UNUSED_SETUP_FD));
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting
    );
    let mut controller = TestProcessController::default();

    let (turn, unregister_calls) = sources_turn(
        &mut supervisor,
        &mut controller,
        vec![RuntimeEventSource::ProcessSetup {
            fd: UNUSED_SETUP_FD,
        }],
        [APP_LAUNCH_NS + 1_000],
    )
    .expect("a per-service error is not a loop error");

    let RuntimeShutdownEventTurn::ProcessSetup {
        fd: UNUSED_SETUP_FD,
        turn: RuntimeProcessSetupTurn::InternalError { dispatch, .. },
    } = &turn.turns[0]
    else {
        panic!("expected the setup to be contained, got {:?}", turn.turns);
    };
    assert_eq!(dispatch.step, "process setup");
    assert_eq!(dispatch.service(), Some("app"));
    assert_eq!(dispatch.subject.job_id, Some(job_id));
    assert_eq!(unregister_calls, vec![UNUSED_SETUP_FD]);
    assert!(!supervisor.has_pending_process_setup(UNUSED_SETUP_FD));
    assert!(supervisor.jobs().get(job_id).is_none());
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Failed);
    assert_eq!(status.cause, Some(TransitionCause::InternalError));
    let operation = supervisor
        .operation_status(operation_id)
        .expect("boot operation");
    assert_eq!(operation.state, OperationState::Failed);
    assert!(
        operation
            .error
            .as_deref()
            .is_some_and(is_internal_error_result),
        "{:?}",
        operation.error
    );
    let messages = console_lines(&turn);
    assert!(
        messages
            .iter()
            .any(|line| line.contains("service app: internal error at process setup")),
        "{messages:?}",
    );
}

/// The control-execution class (PEI-803's mechanism, at the loop level). A
/// Pending Stop is staged for a service with no main job, as if admission
/// had let it through; the work pump's execution refuses it. The loop turn
/// returns with the failure on it, the operation is Failed with the
/// `internal_error` result, the service keeps its state, and the console
/// says which operation failed before it began.
#[test]
fn an_internal_error_on_control_execution_fails_the_operation_not_the_loop() {
    let mut app = alive_service("app");
    app.triggers.clear();
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let operation_id = supervisor
        .operation_ids
        .allocate_batch(1, SHUTDOWN_NS)
        .expect("operation id")[0];
    supervisor
        .operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Stop,
            service: "app".to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: SHUTDOWN_NS,
        })
        .expect("request stop");
    supervisor
        .pending_control_operations
        .push_back(PendingControlOperation {
            operation_id,
            service: "app".to_string(),
            operation_type: OperationType::Stop,
            requirement: PendingControlRequirement::StopProcess,
        });
    let mut controller = TestProcessController::default();

    let turn = loop_turn(&mut supervisor, &mut controller, Vec::new(), Ok(Vec::new()))
        .expect("a refused control operation is not a loop error");

    assert_eq!(turn.pre_work.control_operation_failures.len(), 1);
    assert_eq!(
        turn.pre_work.control_operation_failures[0].operation_id,
        operation_id
    );
    let operation = supervisor
        .operation_status(operation_id)
        .expect("failed operation");
    assert_eq!(operation.state, OperationState::Failed);
    assert!(
        operation
            .error
            .as_deref()
            .is_some_and(is_internal_error_result),
        "{:?}",
        operation.error
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
        "the service keeps the state it had",
    );
    assert!(controller.signals.is_empty());
    let messages = console_lines(&turn);
    assert!(
        messages
            .iter()
            .any(|line| line.contains("service app: Stop operation")
                && line.contains("failed before it began")),
        "{messages:?}",
    );
}

/// The other half of the rule: an error about supervision itself — here the
/// wait — is not attributable to any service, and still ends the loop.
#[test]
fn a_supervision_level_error_still_ends_the_loop() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();

    let error = loop_turn_with_wait(
        &mut supervisor,
        &mut controller,
        Err(RuntimeEventWaitError::Wait(
            crate::boundary::LinuxEpollWaitError::InvalidMaxEvents,
        )),
        Ok(Vec::new()),
    )
    .expect_err("the wait failing is the loop's error");

    assert!(
        matches!(error, RuntimeShutdownLoopError::Wait(_)),
        "{error:?}"
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
        "no service is failed over a supervision-level error",
    );
}

/// One loop turn: `wait` hands over `sources` (or fails), the signal source
/// reads SIGCHLD, and the reaper returns `reaps`.
fn loop_turn(
    supervisor: &mut Supervisor,
    controller: &mut TestProcessController,
    wait: Vec<RuntimeEventSource>,
    reaps: Result<Vec<ChildReap>, crate::boundary::BoundaryError>,
) -> Result<RuntimeShutdownLoopTurn, RuntimeShutdownLoopError> {
    loop_turn_with_wait(supervisor, controller, Ok(wait), reaps)
}

fn loop_turn_with_wait(
    supervisor: &mut Supervisor,
    controller: &mut TestProcessController,
    wait: Result<Vec<RuntimeEventSource>, RuntimeEventWaitError>,
    reaps: Result<Vec<ChildReap>, crate::boundary::BoundaryError>,
) -> Result<RuntimeShutdownLoopTurn, RuntimeShutdownLoopError> {
    let mut waiter = FakeWaiter { wait: Some(wait) };
    let mut signal = FakeSignalSource::new([LinuxSignalFdRead::Other {
        signal: libc::SIGCHLD,
    }]);
    let mut child_reaper = FakeChildReaper::new([reaps]);
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new(vec![SHUTDOWN_NS + 1; 8]);
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(Vec::new());
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();
    let mut jobs_channel = crate::runtime::NoJobsChannel;
    let mut job_identity_provider = crate::runtime::NoJobIdentityProvider;
    process_runtime_shutdown_loop_turn(
        supervisor,
        &mut waiter,
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
        RuntimeShutdownLoopContext {
            clock: &mut clock,
            controller,
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
            job_identity_provider: &mut job_identity_provider,
            jobs_limits: crate::jobs::socket::JobsSocketLimits::default(),
        },
    )
}

/// The sources half of a turn, as the Linux runtime drives it: `sources`
/// are processed against a launcher whose setup-status reader fails.
/// Returns the turn and the descriptors the registrar was asked to drop.
fn sources_turn(
    supervisor: &mut Supervisor,
    controller: &mut TestProcessController,
    sources: Vec<RuntimeEventSource>,
    clock_times: impl Into<std::collections::VecDeque<u64>>,
) -> Result<(RuntimeShutdownLoopTurn, Vec<i32>), RuntimeShutdownLoopError> {
    let mut registry = StaticRegistry::services(Vec::new());
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut clock = ScriptedClock::new(clock_times);
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(Vec::new());
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();
    let mut jobs_channel = crate::runtime::NoJobsChannel;
    let mut job_identity_provider = crate::runtime::NoJobIdentityProvider;
    let turn = process_runtime_shutdown_sources_with_registry(
        supervisor,
        sources,
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
        Some(&mut registry),
        None,
        RuntimeShutdownLoopContext {
            clock: &mut clock,
            controller,
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
            job_identity_provider: &mut job_identity_provider,
            jobs_limits: crate::jobs::socket::JobsSocketLimits::default(),
        },
    )?;
    Ok((turn, registrar.unregister_calls))
}

/// What the turn would put on the console.
fn console_lines(turn: &RuntimeShutdownLoopTurn) -> Vec<String> {
    let mut messages = Vec::new();
    crate::runtime::console::collect_runtime_loop_console_messages(
        &turn.pre_work,
        &turn.turns,
        &turn.post_work,
        &[],
        &mut messages,
    );
    messages.into_iter().map(|message| message.text).collect()
}

/// What the turn would put in the event ring.
#[cfg(feature = "peios-boundary")]
fn kmes_event_types(turn: &RuntimeShutdownLoopTurn) -> Vec<String> {
    let mut events = Vec::new();
    crate::runtime::collect_runtime_loop_kmes_events(
        &turn.pre_work,
        &crate::supervisor::SupervisorOperationMaintenanceTurn::default(),
        &turn.turns,
        &turn.post_work,
        &crate::supervisor::SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut events,
    )
    .expect("collect events");
    events.into_iter().map(|event| event.event_type).collect()
}

#[derive(Debug)]
struct FakeWaiter {
    wait: Option<Result<Vec<RuntimeEventSource>, RuntimeEventWaitError>>,
}

impl RuntimeEventWaiter for FakeWaiter {
    fn wait_runtime_events(
        &mut self,
        _max_events: usize,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventWaitError> {
        self.wait.take().unwrap_or_else(|| Ok(Vec::new()))
    }
}
