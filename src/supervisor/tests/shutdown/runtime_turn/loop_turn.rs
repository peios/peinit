use crate::boundary::{ChildExitStatus, ChildReap, LinuxSignalFdRead};
use crate::control::connection::ControlConnectionTable;
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeEventWaitError, RuntimeEventWaiter,
    RuntimeShutdownEventSources, RuntimeShutdownEventTurn, RuntimeShutdownLoopContext,
    RuntimeWorkPumpConfig, process_runtime_shutdown_loop_turn,
};
use crate::service::runtime::ServiceState;
use crate::shutdown::{ShutdownKind, ShutdownSignal};
use crate::supervisor::{SupervisorChildReapDispatch, SupervisorChildReapTurn};

use super::super::SHUTDOWN_NS;
use super::super::fixture::{DRAINING_STOP_DEADLINE_NS, shutdown_fixture};
use super::support::{
    AllowAccessChecker, DeadlineTimerCall, FakeBootAttemptCounter, FakeChildReaper,
    FakeControlListener, FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource,
    RuntimeFinalizer,
};
use crate::supervisor::tests::{
    ScriptedClock, TestProcessController, TestProcessLauncher, TestTokenProvider,
};

static DEFAULT_CONTROL_SECURITY: crate::control::system::ControlSecurityDescriptor =
    crate::control::system::ControlSecurityDescriptor::Default;

#[test]
fn runtime_shutdown_loop_turn_processes_waited_sources_in_order() {
    let mut supervisor = shutdown_fixture();
    let mut waiter = FakeWaiter::new([
        RuntimeEventSource::Pid1Signal,
        RuntimeEventSource::ShutdownDeadlineTimer,
    ]);
    let mut signal = FakeSignalSource::new([LinuxSignalFdRead::Shutdown(ShutdownSignal::Sigterm)]);
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
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
    let turn = process_runtime_shutdown_loop_turn(
        &mut supervisor,
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
            job_identity_provider: &mut job_identity_provider,
            jobs_limits: crate::jobs::socket::JobsSocketLimits::default(),
        },
    )
    .expect("loop turn");

    assert!(turn.pre_work.is_empty());
    assert!(turn.post_work.is_empty());
    assert_eq!(
        turn.sources,
        vec![
            RuntimeEventSource::Pid1Signal,
            RuntimeEventSource::ShutdownDeadlineTimer,
        ],
    );
    assert!(matches!(
        turn.turns.as_slice(),
        [
            RuntimeShutdownEventTurn::Pid1Signal { .. },
            RuntimeShutdownEventTurn::ShutdownDeadlineTimer { .. },
        ],
    ));
    assert_eq!(waiter.max_events, vec![8]);
    assert_eq!(
        deadline_timer.calls,
        vec![
            DeadlineTimerCall::Arm(DRAINING_STOP_DEADLINE_NS),
            DeadlineTimerCall::Read,
            DeadlineTimerCall::Arm(DRAINING_STOP_DEADLINE_NS),
        ],
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Poweroff,
    );
}

#[test]
fn runtime_shutdown_loop_turn_processes_sigchld_child_reaps() {
    let mut supervisor = shutdown_fixture();
    let mut waiter = FakeWaiter::new([RuntimeEventSource::Pid1Signal]);
    let mut signal = FakeSignalSource::new([LinuxSignalFdRead::Other {
        signal: libc::SIGCHLD,
    }]);
    let mut child_reaper = FakeChildReaper::new([Ok(vec![ChildReap {
        pid: 8000,
        status: ChildExitStatus::Exited { code: 0 },
    }])]);
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS + 1]);
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
    let turn = process_runtime_shutdown_loop_turn(
        &mut supervisor,
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
            job_identity_provider: &mut job_identity_provider,
            jobs_limits: crate::jobs::socket::JobsSocketLimits::default(),
        },
    )
    .expect("loop turn");

    assert!(turn.pre_work.is_empty());
    assert!(turn.post_work.is_empty());
    assert_eq!(turn.sources, vec![RuntimeEventSource::Pid1Signal]);
    assert!(matches!(
        turn.turns.as_slice(),
        [RuntimeShutdownEventTurn::Pid1Signal {
            child_reaps,
            deadline_timer: None,
            ..
        }] if matches!(
            child_reaps.as_slice(),
            [SupervisorChildReapTurn::Tracked {
                child: ChildReap { pid: 8000, .. },
                dispatch: SupervisorChildReapDispatch::Runtime(_),
                ..
            }]
        )
    ));
    assert_eq!(waiter.max_events, vec![8]);
    assert_eq!(child_reaper.calls, 1);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );
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
