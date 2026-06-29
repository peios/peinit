use crate::boundary::{
    BoundaryError, RegistryWatchEvent, RegistryWatchEventKind, RegistryWatchRoot,
    RegistryWatchSource,
};
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeRegistryWatchTurn,
    RuntimeShutdownEventSources, RuntimeShutdownEventTurn, RuntimeShutdownLoopContext,
    RuntimeShutdownLoopTurn, RuntimeWorkPumpConfig, process_runtime_shutdown_sources_with_registry,
};
use crate::supervisor::Supervisor;

use super::super::fixture::shutdown_fixture;
use super::support::{
    AllowAccessChecker, FakeBootAttemptCounter, FakeChildReaper, FakeControlListener,
    FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource, RuntimeFinalizer,
};
use crate::supervisor::tests::{
    ScriptedClock, StaticRegistry, TestProcessController, TestProcessLauncher, TestTokenProvider,
    alive_service,
};

#[test]
fn runtime_registry_watch_event_reloads_config() {
    let mut supervisor = shutdown_fixture();
    let mut registry = StaticRegistry::services(vec![alive_service("fresh")]);
    let mut watch = FakeRegistryWatchSource::events(vec![RegistryWatchEvent {
        root: RegistryWatchRoot::Services,
        kind: RegistryWatchEventKind::ValueSet,
        name: "Description".to_string(),
        path: vec!["fresh".to_string()],
    }]);

    let (turn, _) = run_registry_watch_turn(&mut supervisor, &mut registry, &mut watch);

    let RuntimeShutdownEventTurn::RegistryWatch {
        turn:
            RuntimeRegistryWatchTurn::ReloadConfig {
                events,
                overflow,
                outcome,
            },
        ..
    } = &turn.turns[0]
    else {
        panic!("expected registry-watch reload turn");
    };
    let outcome = outcome.as_ref().as_ref().expect("reload outcome");
    assert_eq!(events.len(), 1);
    assert!(!overflow);
    assert_eq!(outcome.summary.added, vec!["fresh".to_string()]);
    assert!(supervisor.services().get("fresh").is_some());
}

#[test]
fn runtime_registry_watch_overflow_forces_full_reload_config() {
    let mut supervisor = shutdown_fixture();
    let mut registry = StaticRegistry::services(vec![alive_service("fresh")]);
    let mut watch = FakeRegistryWatchSource::events(vec![RegistryWatchEvent {
        root: RegistryWatchRoot::Services,
        kind: RegistryWatchEventKind::Overflow,
        name: String::new(),
        path: Vec::new(),
    }]);

    let (turn, _) = run_registry_watch_turn(&mut supervisor, &mut registry, &mut watch);

    let RuntimeShutdownEventTurn::RegistryWatch {
        turn: RuntimeRegistryWatchTurn::ReloadConfig {
            overflow, outcome, ..
        },
        ..
    } = &turn.turns[0]
    else {
        panic!("expected registry-watch reload turn");
    };
    let outcome = outcome.as_ref().as_ref().expect("reload outcome");
    assert!(*overflow);
    assert_eq!(outcome.summary.added, vec!["fresh".to_string()]);
    assert!(supervisor.services().get("fresh").is_some());
}

#[test]
fn runtime_registry_watch_read_failure_disables_source_without_failing_loop() {
    let mut supervisor = shutdown_fixture();
    let mut registry = StaticRegistry::services(vec![alive_service("fresh")]);
    let mut watch =
        FakeRegistryWatchSource::error(BoundaryError::Registry("watch parse failed".to_string()));

    let (turn, unregister_calls) =
        run_registry_watch_turn(&mut supervisor, &mut registry, &mut watch);

    let RuntimeShutdownEventTurn::RegistryWatch {
        turn: RuntimeRegistryWatchTurn::ReadFailed {
            source_disabled, ..
        },
        ..
    } = &turn.turns[0]
    else {
        panic!("expected registry-watch read failure turn");
    };
    assert!(*source_disabled);
    assert_eq!(unregister_calls, vec![91]);
    assert!(supervisor.services().get("fresh").is_none());
}

fn run_registry_watch_turn(
    supervisor: &mut Supervisor,
    registry: &mut StaticRegistry,
    watch: &mut FakeRegistryWatchSource,
) -> (RuntimeShutdownLoopTurn, Vec<i32>) {
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = crate::control::connection::ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
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

    let turn = process_runtime_shutdown_sources_with_registry(
        supervisor,
        vec![RuntimeEventSource::RegistryWatch { fd: 91 }],
        &mut RuntimeShutdownEventSources {
            signal_source: &mut signal,
            child_reaper: &mut child_reaper,
            notify_source: &mut notify,
            control_listener: &mut listener,
            control_connections: &mut connections,
            deadline_timer: &mut deadline_timer,
            lifecycle_timer: &mut lifecycle_timer,
            filesystem_check_reader: &mut filesystem_check_reader,
            log_pipes: &mut log_pipes,
        },
        Some(registry),
        Some(watch),
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
            control_security: &crate::control::system::ControlSecurityDescriptor::Default,
            max_events: 8,
            control_limits: RuntimeControlLimits::new(
                1024,
                crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
                crate::control::socket::DEFAULT_CONNECTION_TIMEOUT_SECS,
            ),
            work_pump: RuntimeWorkPumpConfig::default(),
        },
    )
    .expect("runtime sources");
    (turn, registrar.unregister_calls)
}

struct FakeRegistryWatchSource {
    result: Result<Vec<RegistryWatchEvent>, BoundaryError>,
}

impl FakeRegistryWatchSource {
    fn events(events: Vec<RegistryWatchEvent>) -> Self {
        Self { result: Ok(events) }
    }

    fn error(error: BoundaryError) -> Self {
        Self { result: Err(error) }
    }
}

impl RegistryWatchSource for FakeRegistryWatchSource {
    fn drain_registry_watch_events(
        &mut self,
        _fd: i32,
    ) -> Result<Vec<RegistryWatchEvent>, BoundaryError> {
        self.result.clone()
    }
}
