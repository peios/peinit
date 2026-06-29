use crate::control::connection::ControlConnectionTable;
use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::execution::control::ControlExecutionDetail;
use crate::runtime::{
    RuntimeEventSource, RuntimeEventWaitError, RuntimeEventWaiter, RuntimeShutdownEventSources,
    RuntimeShutdownEventTurn, process_runtime_shutdown_event,
};
use crate::service::{Readiness, ServiceDefinition};
use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, alive_service, process,
    settings,
};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::super::support::{
    AllowAccessChecker, FakeBootAttemptCounter, FakeChildReaper, FakeControlListener,
    FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource, RuntimeFinalizer,
    context,
};

const CONTROL_NS: u64 = LIFECYCLE_COMMAND_NS + 1;
const RELOAD_COMMAND_LAUNCH_NS: u64 = LIFECYCLE_COMMAND_NS + 2;
const PRE_HOOK_LAUNCH_NS: u64 = BOOT_NS + 10_000;

pub(super) fn process_expired_lifecycle_deadline(
    supervisor: &mut Supervisor,
    now_ns: u64,
) -> (
    RuntimeShutdownEventTurn,
    FakeDeadlineTimer,
    TestProcessController,
) {
    let (turn, timer, controller, _) =
        process_expired_lifecycle_deadline_with_counter_result(supervisor, now_ns, Ok(()));
    (turn, timer, controller)
}

pub(in crate::supervisor::tests::shutdown::runtime_turn) fn process_expired_lifecycle_deadline_with_counter_result(
    supervisor: &mut Supervisor,
    now_ns: u64,
    counter_result: Result<(), crate::boundary::BoundaryError>,
) -> (
    RuntimeShutdownEventTurn,
    FakeDeadlineTimer,
    TestProcessController,
    FakeBootAttemptCounter,
) {
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    let mut shutdown_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::expired_once();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([now_ns]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter {
        result: counter_result,
        ..FakeBootAttemptCounter::default()
    };

    let turn = process_runtime_shutdown_event(
        supervisor,
        RuntimeEventSource::LifecycleDeadlineTimer,
        &mut RuntimeShutdownEventSources {
            signal_source: &mut signal,
            child_reaper: &mut child_reaper,
            notify_source: &mut notify,
            control_listener: &mut listener,
            control_connections: &mut connections,
            deadline_timer: &mut shutdown_timer,
            lifecycle_timer: &mut lifecycle_timer,
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

    (turn, lifecycle_timer, controller, boot_attempt_counter)
}

pub(super) fn notify_app_supervisor() -> Supervisor {
    let app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app_supervisor(app)
}

pub(super) fn active_app_supervisor() -> Supervisor {
    let mut app = alive_service("app");
    app.readiness = Readiness::Alive;
    app_supervisor(app)
}

pub(super) fn active_watchdog_app_supervisor() -> Supervisor {
    let mut app = alive_service("app");
    app.watchdog_timeout_secs = 5;
    app_supervisor(app)
}

pub(super) fn active_app_supervisor_with_reload_command() -> Supervisor {
    let mut app = alive_service("app");
    app.exec_reload = Some(r#"/usr/bin/reload --name="hello world" """#.to_string());
    app.start_timeout_secs = 45;
    app_supervisor(app)
}

fn app_supervisor(app: ServiceDefinition) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot app");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    supervisor
}

pub(super) fn pre_start_hook_supervisor() -> (Supervisor, crate::ids::JobId) {
    let mut app = alive_service("app");
    app.exec_start_pre = vec!["/usr/bin/pre".to_string()];
    app.start_timeout_secs = 45;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let hook_job = boot.start_dispatches[0].job_id;

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6100, 71)]);
    let mut hook_clock = ScriptedClock::new([PRE_HOOK_LAUNCH_NS]);
    supervisor
        .launch_next_pending_start_hook_job(&mut tokens, &mut launcher, &mut hook_clock)
        .expect("launch hook")
        .expect("hook launch dispatch");

    (supervisor, hook_job)
}

pub(super) fn stop_app(supervisor: &mut Supervisor) -> crate::ids::OperationId {
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .stop_service("app", None, &mut clock)
        .expect("stop app");
    accepted_operation(accepted.outcome)
}

pub(super) fn reload_app(supervisor: &mut Supervisor) -> crate::ids::OperationId {
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .reload_service("app", None, &mut clock)
        .expect("reload app");
    accepted_operation(accepted.outcome)
}

fn accepted_operation(outcome: LifecycleCommandOutcome) -> crate::ids::OperationId {
    let LifecycleCommandOutcome::OperationAccepted(operation) = outcome else {
        panic!("expected accepted operation");
    };
    operation.returned_operation_id
}

pub(super) fn execute_next_control_operation(supervisor: &mut Supervisor) {
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute control")
        .expect("control dispatch");
}

pub(super) fn execute_and_launch_reload_command(supervisor: &mut Supervisor) -> crate::ids::JobId {
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    let dispatch = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload dispatch");
    let ControlExecutionDetail::ReloadCommand { job_id, .. } = dispatch.execution.detail else {
        panic!("expected reload command");
    };

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9100, 61)]);
    let mut launch_clock = ScriptedClock::new([RELOAD_COMMAND_LAUNCH_NS]);
    supervisor
        .launch_next_pending_control_job(&mut tokens, &mut launcher, &mut launch_clock)
        .expect("launch reload command")
        .expect("reload command launch");

    job_id
}

#[derive(Debug)]
pub(super) struct FakeWaiter {
    sources: Vec<RuntimeEventSource>,
    pub(super) max_events: Vec<usize>,
}

impl FakeWaiter {
    pub(super) fn new(sources: impl IntoIterator<Item = RuntimeEventSource>) -> Self {
        Self {
            sources: sources.into_iter().collect(),
            max_events: Vec::new(),
        }
    }
}

impl RuntimeEventWaiter for FakeWaiter {
    fn wait_runtime_events(
        &mut self,
        max_events: usize,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventWaitError> {
        self.max_events.push(max_events);
        Ok(std::mem::take(&mut self.sources))
    }
}
