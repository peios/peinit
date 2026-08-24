use crate::control::connection::ControlConnectionTable;
use crate::execution::notify::{NotifyAppliedField, NotifyApplyError};
use crate::notify::NotifySocketReadError;
use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::runtime::RuntimeShutdownEventTurnError;
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeEventWaitError, RuntimeEventWaiter,
    RuntimeNotifyRead, RuntimeNotifyRejection, RuntimeNotifySupervisorTurn,
    RuntimeShutdownEventSources, RuntimeShutdownEventTurn, RuntimeShutdownLoopContext,
    RuntimeShutdownLoopTurn, RuntimeWorkPumpConfig, process_runtime_shutdown_event,
    process_runtime_shutdown_loop_turn,
};
use crate::service::runtime::ServiceState;
use crate::service::{Readiness, ServiceDefinition};
use crate::shutdown::{ShutdownDeadlineKind, ShutdownKind};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::super::SHUTDOWN_NS;
use super::support::{
    AllowAccessChecker, DeadlineTimerCall, FakeBootAttemptCounter, FakeChildReaper,
    FakeControlListener, FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource,
    RuntimeFinalizer, context,
};
use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, process, settings,
};

const NOTIFY_NS: u64 = SHUTDOWN_NS + 1_000;
const EXTEND_NS: u64 = SHUTDOWN_NS + 1_000_000;
const EXTENDED_APP_STOP_DEADLINE_NS: u64 = SHUTDOWN_NS + 40_000_000_000;

static DEFAULT_CONTROL_SECURITY: crate::control::system::ControlSecurityDescriptor =
    crate::control::system::ControlSecurityDescriptor::Default;

#[test]
fn runtime_loop_notify_event_applies_ready_datagram() {
    let mut supervisor = notify_app_supervisor();
    let result = run_notify_loop_turn(
        &mut supervisor,
        NOTIFY_NS,
        datagram(8000, b"STATUS=Listening\nREADY=1"),
    );

    assert!(result.turn.pre_work.is_empty());
    assert!(result.turn.post_work.is_empty());
    assert_eq!(result.turn.sources, vec![RuntimeEventSource::NotifySocket]);
    assert!(matches!(
        result.turn.turns.as_slice(),
        [RuntimeShutdownEventTurn::Notify {
            read: RuntimeNotifyRead::Datagram(read),
            supervisor: Some(RuntimeNotifySupervisorTurn::Applied(dispatch)),
            deadline_timer: None,
        }] if read.sender_pid == 8000
            && read.payload == b"STATUS=Listening\nREADY=1"
            && dispatch.notify.applied_fields == vec![
                NotifyAppliedField::Status {
                    text: "Listening".to_string(),
                },
                NotifyAppliedField::Ready,
            ]
    ));
    assert_eq!(result.waiter_max_events, vec![8]);
    assert_eq!(result.notify_calls, 1);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor
            .service_status("app")
            .expect("app")
            .status_text
            .as_deref(),
        Some("Listening"),
    );
    assert!(supervisor.next_readiness_timeout().is_none());
}

#[test]
fn runtime_notify_event_extends_shutdown_deadline_and_resyncs_timer() {
    let mut supervisor = active_app_supervisor();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    let result = run_notify_event(
        &mut supervisor,
        EXTEND_NS,
        datagram(8000, b"EXTEND_TIMEOUT_USEC=1000000000"),
    );

    let RuntimeShutdownEventTurn::Notify {
        supervisor: Some(RuntimeNotifySupervisorTurn::Applied(dispatch)),
        deadline_timer:
            Some(crate::supervisor::SupervisorShutdownDeadlineTimerTurn::Armed { deadline }),
        ..
    } = result.turn
    else {
        panic!("expected applied notify and rearmed deadline timer");
    };
    assert_eq!(
        dispatch.notify.applied_fields,
        vec![NotifyAppliedField::ExtendTimeoutUsec {
            value: "1000000000".to_string(),
        }],
    );
    assert_eq!(deadline.due_at_ns, EXTENDED_APP_STOP_DEADLINE_NS);
    assert_eq!(
        deadline.kind,
        ShutdownDeadlineKind::StopTimeout {
            service: "app".to_string(),
        },
    );
    assert_eq!(
        result.deadline_timer.calls,
        vec![DeadlineTimerCall::Arm(EXTENDED_APP_STOP_DEADLINE_NS)],
    );
    assert_eq!(
        supervisor
            .shutdown()
            .expect("shutdown")
            .stop_deadlines
            .iter()
            .find(|deadline| deadline.service == "app")
            .expect("app deadline")
            .due_at_ns,
        EXTENDED_APP_STOP_DEADLINE_NS,
    );
}

#[test]
fn runtime_notify_event_rejects_unauthenticated_datagram_without_failing_loop() {
    let mut supervisor = notify_app_supervisor();
    let result = run_notify_event(&mut supervisor, NOTIFY_NS, datagram(9999, b"READY=1"));

    assert!(matches!(
        result.turn,
        RuntimeShutdownEventTurn::Notify {
            supervisor: Some(RuntimeNotifySupervisorTurn::Rejected(
                RuntimeNotifyRejection::Apply {
                    error: NotifyApplyError::UnauthenticatedSender { pid: 9999 },
                    attribution: None,
                },
            )),
            deadline_timer: None,
            ..
        }
    ));
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
}

#[test]
fn runtime_notify_parse_rejection_retains_authenticated_attribution() {
    let mut supervisor = notify_app_supervisor();
    let result = run_notify_event(
        &mut supervisor,
        NOTIFY_NS,
        datagram(8000, b"STATUS=Listening\nbroken\nREADY=1"),
    );

    let RuntimeShutdownEventTurn::Notify {
        supervisor:
            Some(RuntimeNotifySupervisorTurn::Rejected(RuntimeNotifyRejection::Parse {
                error,
                attribution: Some(attribution),
            })),
        deadline_timer: None,
        ..
    } = result.turn
    else {
        panic!("expected attributed parse rejection");
    };
    assert_eq!(
        error,
        crate::notify::NotifyParseError::MalformedLine { line_index: 1 },
    );
    assert_eq!(attribution.service, "app");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").status_text,
        None,
    );
}

struct NotifyEventResult {
    turn: RuntimeShutdownEventTurn,
    deadline_timer: FakeDeadlineTimer,
}

/// §1.4's all-or-nothing rule: a datagram that is not delivered whole has no
/// fields applied from it.
///
/// peinit implemented that faithfully against a malformed *line* and was blind
/// to a truncated one — the case it cannot see, because a truncated tail can
/// parse as a complete, valid KEY=VALUE line. A long enough STATUS= produced a
/// datagram whose last line was wherever the cut landed, and if the cut fell
/// after an `=` it was well-formed and was applied.
///
/// It has to arrive as a *rejection*, not as a read error: the datagram was
/// consumed and is gone, so ending the runtime loop over it would be a worse
/// answer than the silent truncation it replaces.
#[test]
fn a_truncated_notify_datagram_is_rejected_rather_than_applied() {
    for (payload, control) in [(true, false), (false, true), (true, true)] {
        let mut supervisor = notify_app_supervisor();
        let result = run_notify_read(
            &mut supervisor,
            NOTIFY_NS,
            Err(NotifySocketReadError::Truncated { payload, control }),
        );

        let RuntimeShutdownEventTurn::Notify {
            supervisor:
                Some(RuntimeNotifySupervisorTurn::Rejected(RuntimeNotifyRejection::Truncated {
                    payload: got_payload,
                    control: got_control,
                })),
            ..
        } = result.turn
        else {
            panic!("expected a truncation rejection, got {:?}", result.turn);
        };
        assert_eq!((got_payload, got_control), (payload, control));
    }
}

/// And an ordinary read failure is still a read failure — the truncation path
/// must not swallow every error into a rejection.
#[test]
fn a_read_failure_is_still_a_read_failure() {
    let mut supervisor = notify_app_supervisor();
    let outcome = try_notify_read(
        &mut supervisor,
        NOTIFY_NS,
        Err(NotifySocketReadError::MissingCredentials),
    );
    assert!(
        outcome.is_err(),
        "a socket-level failure must not be reported as a rejected datagram"
    );
}

fn run_notify_event(
    supervisor: &mut Supervisor,
    now_ns: u64,
    datagram: NotifyDatagram,
) -> NotifyEventResult {
    run_notify_read(supervisor, now_ns, Ok(Some(datagram)))
}

/// [`run_notify_event`] over a raw socket result, so a test can drive an error.
fn run_notify_read(
    supervisor: &mut Supervisor,
    now_ns: u64,
    read: Result<Option<NotifyDatagram>, NotifySocketReadError>,
) -> NotifyEventResult {
    try_notify_read(supervisor, now_ns, read).expect("runtime event")
}

fn try_notify_read(
    supervisor: &mut Supervisor,
    now_ns: u64,
    read: Result<Option<NotifyDatagram>, NotifySocketReadError>,
) -> Result<NotifyEventResult, RuntimeShutdownEventTurnError> {
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::new([read]);
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([now_ns]);
    let mut controller = TestProcessController::default();
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();

    let turn = process_runtime_shutdown_event(
        supervisor,
        RuntimeEventSource::NotifySocket,
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
    )?;

    Ok(NotifyEventResult {
        turn,
        deadline_timer,
    })
}

struct NotifyLoopResult {
    turn: RuntimeShutdownLoopTurn,
    waiter_max_events: Vec<usize>,
    notify_calls: usize,
}

fn run_notify_loop_turn(
    supervisor: &mut Supervisor,
    now_ns: u64,
    datagram: NotifyDatagram,
) -> NotifyLoopResult {
    let mut waiter = FakeWaiter::new([RuntimeEventSource::NotifySocket]);
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::new([Ok(Some(datagram))]);
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([now_ns]);
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

    NotifyLoopResult {
        turn,
        waiter_max_events: waiter.max_events,
        notify_calls: notify.calls,
    }
}

fn notify_app_supervisor() -> Supervisor {
    let app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app_supervisor(app)
}

fn active_app_supervisor() -> Supervisor {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.readiness = Readiness::Alive;
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

fn datagram(pid: u32, payload: &[u8]) -> NotifyDatagram {
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
