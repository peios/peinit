use crate::boundary::{
    BoundaryError, RegistryWatchEvent, RegistryWatchEventKind, RegistryWatchRoot,
    RegistryWatchSource,
};
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeRegistryWatchTurn,
    RuntimeShutdownEventSources, RuntimeShutdownEventTurn, RuntimeShutdownLoopContext,
    RuntimeShutdownLoopTurn, RuntimeWorkPumpConfig, process_runtime_shutdown_sources_with_registry,
};
use crate::service::runtime::ServiceState;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::super::fixture::shutdown_fixture;
use super::support::{
    AllowAccessChecker, FakeBootAttemptCounter, FakeChildReaper, FakeControlListener,
    FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource, RuntimeFinalizer,
};
use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
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
    run_registry_watch_turn_launching(supervisor, registry, watch, Vec::new(), Vec::new())
}

/// The registry-watch turn, with the work pump able to launch: the clock
/// readings and processes the launches it makes will consume.
fn run_registry_watch_turn_launching(
    supervisor: &mut Supervisor,
    registry: &mut StaticRegistry,
    watch: &mut FakeRegistryWatchSource,
    clock_times: Vec<u64>,
    processes: Vec<crate::boundary::LaunchedProcess>,
) -> (RuntimeShutdownLoopTurn, Vec<i32>) {
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
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(processes);
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let mut jobs_channel = crate::runtime::NoJobsChannel;
    let mut job_identity_provider = crate::runtime::NoJobIdentityProvider;
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
            power_button_source: &mut power_button,
            filesystem_check_reader: &mut filesystem_check_reader,
            log_pipes: &mut log_pipes,
            jobs_channel: &mut jobs_channel,
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
            job_identity_provider: &mut job_identity_provider,
            jobs_limits: crate::jobs::socket::JobsSocketLimits::default(),
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

/// PEI-350 (§3.7): a watch event during the boot window reloads, but a
/// boot-plan member whose launch has not been attempted keeps the plan's
/// definition with the new one pending; the reload after the window — here
/// the same turn, because the work pump after the sources launches the last
/// member — applies it, announced as the coalesced reload.
#[test]
fn runtime_registry_watch_event_during_the_boot_window_defers_the_unlaunched_member() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut boot_registry = StaticRegistry::services(vec![alive_service("app")]);
    supervisor
        .run_phase2_boot(&mut boot_registry, &mut ScriptedClock::new([BOOT_NS]))
        .expect("boot");
    assert!(supervisor.boot_plan_in_progress());
    let mut changed_app = alive_service("app");
    changed_app.image_path = "/sbin/app-v2".to_string();
    let mut registry = StaticRegistry::services(vec![changed_app, alive_service("fresh")]);
    let mut watch = FakeRegistryWatchSource::events(vec![RegistryWatchEvent {
        root: RegistryWatchRoot::Services,
        kind: RegistryWatchEventKind::ValueSet,
        name: "ImagePath".to_string(),
        path: vec!["app".to_string()],
    }]);

    let (turn, _) = run_registry_watch_turn_launching(
        &mut supervisor,
        &mut registry,
        &mut watch,
        vec![APP_LAUNCH_NS],
        vec![process(4242, 9)],
    );

    // First the watch event: the reload ran, added `fresh`, and deferred
    // `app`, whose launch had not been attempted when it was read...
    let RuntimeShutdownEventTurn::RegistryWatch {
        fd: 91,
        turn: RuntimeRegistryWatchTurn::ReloadConfig { outcome, .. },
    } = &turn.turns[0]
    else {
        panic!(
            "expected a registry-watch reload turn, got {:?}",
            turn.turns
        );
    };
    let outcome = outcome.as_ref().as_ref().expect("reload outcome");
    assert_eq!(outcome.summary.added, vec!["fresh".to_string()]);
    assert_eq!(outcome.summary.deferred, vec!["app".to_string()]);
    // ...then the pump launched app from the plan's definition (Alive
    // readiness: Active at once, the window closed), and the coalesced
    // reload followed.
    assert_eq!(turn.post_work.service_launches.len(), 1);
    assert!(!supervisor.boot_plan_in_progress());
    let RuntimeShutdownEventTurn::DeferredRegistryReload { turn: reload } = &turn.turns[1] else {
        panic!("expected the coalesced reload, got {:?}", turn.turns);
    };
    assert_eq!(turn.turns.len(), 2);
    assert_eq!(
        reload.deferred,
        crate::supervisor::DeferredRegistryReload {
            services: vec!["app".to_string()],
        }
    );
    let coalesced = reload.outcome.as_ref().as_ref().expect("coalesced outcome");
    assert!(coalesced.summary.deferred.is_empty());
    let app = supervisor.services().get("app").expect("app");
    assert_eq!(app.runtime.state, ServiceState::Active);
    assert_eq!(app.definition.image_path, "/sbin/app");
    assert_eq!(
        app.pending_definition
            .as_ref()
            .map(|d| d.image_path.as_str()),
        Some("/sbin/app-v2")
    );
    assert!(supervisor.services().get("fresh").is_some());
    assert!(!supervisor.has_deferred_registry_reload());

    // Once, not again.
    let mut quiet_watch = FakeRegistryWatchSource::events(Vec::new());
    let (turn, _) = run_registry_watch_turn(&mut supervisor, &mut registry, &mut quiet_watch);
    assert_eq!(turn.turns.len(), 1);
    assert!(matches!(
        &turn.turns[0],
        RuntimeShutdownEventTurn::RegistryWatch {
            turn: RuntimeRegistryWatchTurn::NoEvents { .. },
            ..
        }
    ));
}
