//! A process-setup readiness for a descriptor the supervisor no longer
//! holds a setup for (PEI-826).

use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeProcessSetupTurn, RuntimeShutdownEventSources,
    RuntimeShutdownEventTurn, RuntimeShutdownLoopContext, RuntimeShutdownLoopTurn,
    RuntimeWorkPumpConfig, process_runtime_shutdown_sources_with_registry,
};
use crate::supervisor::Supervisor;

use super::super::fixture::shutdown_fixture;
use super::support::{
    AllowAccessChecker, FakeBootAttemptCounter, FakeChildReaper, FakeControlListener,
    FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource, RuntimeFinalizer,
};
use crate::supervisor::tests::{
    ScriptedClock, StaticRegistry, TestProcessController, TestProcessLauncher, TestTokenProvider,
};

/// A shutdown that begins earlier in a turn cancels a Starting job's pending
/// setup and closes its descriptor; the readiness epoll already returned for
/// that descriptor is still in the batch. It is not read -- the test
/// launcher's reader fails for any descriptor, as `read(2)` on a closed one
/// would -- and it does not end the loop: the registration is dropped and
/// the event is reported stale.
#[test]
fn a_readiness_for_a_setup_the_supervisor_dropped_is_stale_rather_than_read() {
    let mut supervisor = shutdown_fixture();
    let mut registry = StaticRegistry::services(Vec::new());
    assert!(!supervisor.has_pending_process_setup(53));

    let (turn, unregister_calls) = run_process_setup_turn(&mut supervisor, &mut registry, 53);

    assert!(matches!(
        &turn.turns[0],
        RuntimeShutdownEventTurn::ProcessSetup {
            fd: 53,
            turn: RuntimeProcessSetupTurn::Stale { fd: 53 },
        }
    ));
    assert_eq!(unregister_calls, vec![53]);
}

fn run_process_setup_turn(
    supervisor: &mut Supervisor,
    registry: &mut StaticRegistry,
    fd: i32,
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

    let mut jobs_channel = crate::runtime::NoJobsChannel;
    let mut job_identity_provider = crate::runtime::NoJobIdentityProvider;
    let turn = process_runtime_shutdown_sources_with_registry(
        supervisor,
        vec![RuntimeEventSource::ProcessSetup { fd }],
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
        None,
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
