use std::collections::VecDeque;
use std::fs::File;
use std::os::fd::FromRawFd;

use crate::boundary::{
    BoundaryError, FilesystemCheckHelperRequest, FilesystemCheckReport,
    LaunchedFilesystemCheckHelper, LaunchedProcess,
};
use crate::control::connection::ControlConnectionTable;
use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::logging::ServiceLogRecord;
use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeEventWaitError, RuntimeEventWaiter,
    RuntimeShutdownEventSources, RuntimeShutdownLoopContext, RuntimeShutdownLoopTurn,
    RuntimeWorkPumpConfig, process_runtime_shutdown_loop_turn,
};
use crate::service::ServiceDefinition;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::super::support::{
    AllowAccessChecker, FakeBootAttemptCounter, FakeControlListener, FakeRegistrar, RegistrarCall,
    RuntimeFinalizer,
};
pub(super) use super::super::support::{
    FakeChildReaper, FakeDeadlineTimer, FakeNotifySource, FakeSignalSource,
};
use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, settings,
};

pub(super) const CONTROL_NS: u64 = LIFECYCLE_COMMAND_NS + 1;
pub(super) const NOTIFY_NS: u64 = BOOT_NS + 30_000;
pub(super) const DEPENDENT_LAUNCH_NS: u64 = NOTIFY_NS + 10_000;
pub(super) const PRE_HOOK_LAUNCH_NS: u64 = BOOT_NS + 10_000;
pub(super) const PRE_HOOK_DONE_NS: u64 = BOOT_NS + 20_000;
pub(super) const MAIN_LAUNCH_NS: u64 = BOOT_NS + 50_000;
pub(super) const RELOAD_COMMAND_LAUNCH_NS: u64 = LIFECYCLE_COMMAND_NS + 2;

static DEFAULT_CONTROL_SECURITY: crate::control::system::ControlSecurityDescriptor =
    crate::control::system::ControlSecurityDescriptor::Default;

#[derive(Debug)]
pub(super) struct LoopScript {
    clock_times: VecDeque<u64>,
    processes: Vec<LaunchedProcess>,
    events: Vec<RuntimeEventSource>,
    signal: FakeSignalSource,
    child_reaper: FakeChildReaper,
    notify: FakeNotifySource,
    lifecycle_timer: FakeDeadlineTimer,
    filesystem_reports: VecDeque<Result<Option<FilesystemCheckReport>, BoundaryError>>,
    filesystem_result_fd: i32,
}

impl LoopScript {
    pub(super) fn new(
        clock_times: impl Into<VecDeque<u64>>,
        processes: impl IntoIterator<Item = LaunchedProcess>,
    ) -> Self {
        Self {
            clock_times: clock_times.into(),
            processes: processes.into_iter().collect(),
            events: Vec::new(),
            signal: FakeSignalSource::would_block(),
            child_reaper: FakeChildReaper::empty(),
            notify: FakeNotifySource::empty(),
            lifecycle_timer: FakeDeadlineTimer::would_block(),
            filesystem_reports: VecDeque::new(),
            filesystem_result_fd: 81,
        }
    }

    pub(super) fn events(mut self, events: impl IntoIterator<Item = RuntimeEventSource>) -> Self {
        self.events = events.into_iter().collect();
        self
    }

    pub(super) fn signal(mut self, signal: FakeSignalSource) -> Self {
        self.signal = signal;
        self
    }

    pub(super) fn child_reaper(mut self, child_reaper: FakeChildReaper) -> Self {
        self.child_reaper = child_reaper;
        self
    }

    pub(super) fn notify(mut self, notify: FakeNotifySource) -> Self {
        self.notify = notify;
        self
    }

    pub(super) fn lifecycle_timer(mut self, lifecycle_timer: FakeDeadlineTimer) -> Self {
        self.lifecycle_timer = lifecycle_timer;
        self
    }

    pub(super) fn filesystem_reports(
        mut self,
        reports: impl Into<VecDeque<Result<Option<FilesystemCheckReport>, BoundaryError>>>,
    ) -> Self {
        self.filesystem_reports = reports.into();
        self
    }
}

#[derive(Debug)]
pub(super) struct LoopResult {
    pub(super) turn: RuntimeShutdownLoopTurn,
    pub(super) token_jobs: Vec<String>,
    pub(super) launched_jobs: Vec<String>,
    pub(super) controller: TestProcessController,
    pub(super) registrar_calls: Vec<RegistrarCall>,
    pub(super) registrar_unregister_calls: Vec<i32>,
    pub(super) buffered_logs: Vec<ServiceLogRecord>,
    pub(super) active_log_pipe_count: usize,
    pub(super) filesystem_check_requests: Vec<FilesystemCheckHelperRequest>,
    pub(super) filesystem_check_reader_helpers: Vec<LaunchedFilesystemCheckHelper>,
    pub(super) filesystem_check_released_fds: Vec<(i32, i32)>,
}

pub(super) fn run_loop(supervisor: &mut Supervisor, script: LoopScript) -> LoopResult {
    let mut waiter = FakeWaiter::new(script.events);
    let mut signal = script.signal;
    let mut child_reaper = script.child_reaper;
    let mut notify = script.notify;
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    let mut shutdown_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = script.lifecycle_timer;
    let mut power_button = super::super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::new(script.filesystem_reports);
    let mut clock = ScriptedClock::new(script.clock_times);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(script.processes);
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default()
            .result_fd(script.filesystem_result_fd);
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_loop_turn(
        supervisor,
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
    .expect("runtime loop turn");

    assert_eq!(waiter.max_events, vec![8]);
    let registrar_calls = registrar.calls.clone();
    let registrar_unregister_calls = registrar.unregister_calls.clone();
    let buffered_logs = log_pipes.buffered_records();
    let active_log_pipe_count = log_pipes.active_pipe_count();
    let filesystem_check_requests = filesystem_check_launcher.requests.clone();
    let filesystem_check_reader_helpers = filesystem_check_reader.helpers.clone();
    let filesystem_check_released_fds = filesystem_check_reader.released_fds.clone();

    LoopResult {
        turn,
        token_jobs: tokens.observed_jobs,
        launched_jobs: launcher.observed_jobs,
        controller,
        registrar_calls,
        registrar_unregister_calls,
        buffered_logs,
        active_log_pipe_count,
        filesystem_check_requests,
        filesystem_check_reader_helpers,
        filesystem_check_released_fds,
    }
}

pub(super) fn boot_supervisor(services: Vec<ServiceDefinition>) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(services);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
}

pub(super) fn boot_supervisor_with_eventd_log_socket_path(
    services: Vec<ServiceDefinition>,
    path: &str,
) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(services).with_eventd_log_socket_path(path);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
}

pub(super) fn active_app_supervisor(
    app: ServiceDefinition,
    process: LaunchedProcess,
) -> Supervisor {
    let mut supervisor = boot_supervisor(vec![app]);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");
    supervisor
}

pub(super) fn queue_reload(supervisor: &mut Supervisor) -> crate::ids::OperationId {
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .reload_service("app", None, &mut clock)
        .expect("reload app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = accepted.outcome else {
        panic!("expected reload operation");
    };
    operation.returned_operation_id
}

pub(super) fn datagram(pid: u32, payload: &[u8]) -> NotifyDatagram {
    NotifyDatagram {
        payload: payload.to_vec(),
        credentials: NotifyCredentials {
            pid,
            uid: 0,
            gid: 0,
        },
        fds: Vec::new(),
    }
}

pub(super) fn pipe_pair() -> (File, File) {
    let mut fds = [0_i32; 2];
    let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    assert_eq!(
        result,
        0,
        "pipe2 failed: {}",
        std::io::Error::last_os_error()
    );
    unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
}

#[derive(Debug)]
struct FakeWaiter {
    sources: Vec<RuntimeEventSource>,
    max_events: Vec<usize>,
}

impl FakeWaiter {
    fn new(sources: impl IntoIterator<Item = RuntimeEventSource>) -> Self {
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
