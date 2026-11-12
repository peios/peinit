//! A datagram that arrives before its sender's setup status is processed
//! (PEI-1688).
//!
//! A job records its PID only once its setup status is processed, and the
//! status and the datagram are separate sources that a turn handles in epoll
//! order. lpsd sends `READY=1` within milliseconds of exec, and on about half
//! of boots its datagram was handled first: refused as an unauthenticated
//! sender, never sent again, and the service was killed at its start timeout.
//! The runtime now reads the sender's setup status before the datagram.

use crate::boundary::ProcessSetupStatus;
use crate::control::connection::ControlConnectionTable;
use crate::execution::notify::{NotifyAppliedField, NotifyApplyError};
use crate::ids::JobId;
use crate::job::JobState;
use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeHeldNotifyDatagrams, RuntimeNotifyRead,
    RuntimeNotifyRejection, RuntimeNotifySupervisorTurn, RuntimeProcessSetupTurn,
    RuntimeShutdownEventSources, RuntimeShutdownEventTurn, RuntimeShutdownLoopContext,
    RuntimeShutdownLoopTurn, RuntimeWorkPumpConfig, process_runtime_shutdown_sources_with_registry,
};
use crate::service::ServiceDefinition;
use crate::service::runtime::ServiceState;
use crate::supervisor::{Supervisor, SupervisorServiceLaunchDispatch, SupervisorSettings};

use super::support::{
    AllowAccessChecker, FakeBootAttemptCounter, FakeChildReaper, FakeControlListener,
    FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource, RuntimeFinalizer,
};
use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, process, settings,
};

const APP_PID: u32 = 4242;
const APP_PIDFD: i32 = 9;
const SETUP_FD: i32 = 55;
const TURN_NS: u64 = APP_LAUNCH_NS + 1_000;

/// The regression itself: `READY=1` handled before the ExecSucceeded that
/// gives the job its PID, in one batch.
#[test]
fn ready_handled_before_its_setup_status_makes_the_service_active() {
    let (mut supervisor, job_id) = app_in_pending_setup();
    let mut harness = Harness::new([ProcessSetupStatus::ExecSucceeded]);
    harness.notify(datagram(APP_PID, b"READY=1"));

    let turn = harness.run(
        &mut supervisor,
        vec![
            RuntimeEventSource::NotifySocket,
            RuntimeEventSource::ProcessSetup { fd: SETUP_FD },
        ],
    );

    // The setup status is read first, once, and the datagram applied after
    // it; the batch's own readiness for the descriptor is not read again.
    assert!(
        matches!(
            turn.turns.as_slice(),
            [
                RuntimeShutdownEventTurn::ProcessSetup {
                    fd: SETUP_FD,
                    turn: RuntimeProcessSetupTurn::Completed { .. },
                },
                RuntimeShutdownEventTurn::Notify {
                    read: RuntimeNotifyRead::Datagram(read),
                    supervisor: Some(RuntimeNotifySupervisorTurn::Applied(dispatch)),
                    ..
                },
            ] if read.sender_pid == APP_PID
                && dispatch.notify.applied_fields == vec![NotifyAppliedField::Ready]
        ),
        "{:#?}",
        turn.turns,
    );
    assert_eq!(harness.launcher.read_setup_fds, vec![SETUP_FD]);
    assert_eq!(
        supervisor.jobs().get(job_id).expect("job").state,
        JobState::Running
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert!(supervisor.next_readiness_timeout().is_none());
}

/// The setup's readiness need not be in the same batch: epoll can return the
/// datagram alone, and the status is read on its account.
#[test]
fn ready_is_applied_when_its_setup_status_is_not_in_the_batch() {
    let (mut supervisor, _job_id) = app_in_pending_setup();
    let mut harness = Harness::new([ProcessSetupStatus::ExecSucceeded]);
    harness.notify(datagram(APP_PID, b"READY=1"));

    let turn = harness.run(&mut supervisor, vec![RuntimeEventSource::NotifySocket]);

    assert!(
        matches!(
            turn.turns.as_slice(),
            [
                RuntimeShutdownEventTurn::ProcessSetup {
                    turn: RuntimeProcessSetupTurn::Completed { .. },
                    ..
                },
                RuntimeShutdownEventTurn::Notify {
                    supervisor: Some(RuntimeNotifySupervisorTurn::Applied(_)),
                    ..
                },
            ]
        ),
        "{:#?}",
        turn.turns,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
}

/// Reading ahead is only for a launched process: a PID with no job and no
/// pending setup is refused as it always was, and no setup is read for it.
#[test]
fn a_sender_with_no_job_and_no_pending_setup_is_still_unauthenticated() {
    let (mut supervisor, job_id) = app_in_pending_setup();
    let mut harness = Harness::new([ProcessSetupStatus::ExecSucceeded]);
    harness.notify(datagram(9999, b"READY=1"));

    let turn = harness.run(&mut supervisor, vec![RuntimeEventSource::NotifySocket]);

    assert!(
        matches!(
            turn.turns.as_slice(),
            [RuntimeShutdownEventTurn::Notify {
                supervisor: Some(RuntimeNotifySupervisorTurn::Rejected(
                    RuntimeNotifyRejection::Apply {
                        error: NotifyApplyError::UnauthenticatedSender { pid: 9999 },
                        attribution: None,
                    }
                )),
                ..
            }]
        ),
        "{:#?}",
        turn.turns,
    );
    assert!(harness.launcher.read_setup_fds.is_empty());
    assert!(harness.notify.held.is_empty());
    assert_eq!(
        supervisor.jobs().get(job_id).expect("job").state,
        JobState::Created,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
}

/// The status cannot be read yet: the datagram is held, not dropped, and
/// applied in the turn that processes the setup.
#[test]
fn a_datagram_whose_setup_status_is_not_readable_is_held_until_it_is() {
    let (mut supervisor, _job_id) = app_in_pending_setup();
    let mut harness = Harness::new([
        ProcessSetupStatus::Pending,
        ProcessSetupStatus::ExecSucceeded,
    ]);
    harness.notify(datagram(APP_PID, b"READY=1"));

    let first = harness.run(&mut supervisor, vec![RuntimeEventSource::NotifySocket]);

    assert!(
        matches!(
            first.turns.as_slice(),
            [
                RuntimeShutdownEventTurn::ProcessSetup {
                    turn: RuntimeProcessSetupTurn::Pending { .. },
                    ..
                },
                RuntimeShutdownEventTurn::Notify {
                    supervisor: Some(RuntimeNotifySupervisorTurn::HeldUntilSetup {
                        sender_pid: APP_PID,
                    }),
                    ..
                },
            ]
        ),
        "{:#?}",
        first.turns,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );

    let second = harness.run(
        &mut supervisor,
        vec![RuntimeEventSource::ProcessSetup { fd: SETUP_FD }],
    );

    assert!(
        matches!(
            second.turns.as_slice(),
            [
                RuntimeShutdownEventTurn::ProcessSetup {
                    turn: RuntimeProcessSetupTurn::Completed { .. },
                    ..
                },
                RuntimeShutdownEventTurn::Notify {
                    read: RuntimeNotifyRead::Datagram(read),
                    supervisor: Some(RuntimeNotifySupervisorTurn::Applied(_)),
                    ..
                },
            ] if read.sender_pid == APP_PID
        ),
        "{:#?}",
        second.turns,
    );
    assert!(harness.notify.held.is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
}

/// A sender's held datagrams are applied before a later one from it, so
/// that `READY=1` then `STATUS=` cannot arrive at the service reversed.
#[test]
fn held_datagrams_are_applied_before_a_later_one_from_the_same_sender() {
    let (mut supervisor, _job_id) = app_in_pending_setup();
    let mut harness = Harness::new([
        ProcessSetupStatus::Pending,
        ProcessSetupStatus::ExecSucceeded,
    ]);
    harness.notify(datagram(APP_PID, b"STATUS=first"));
    harness.run(&mut supervisor, vec![RuntimeEventSource::NotifySocket]);
    harness.notify(datagram(APP_PID, b"STATUS=second\nREADY=1"));

    let turn = harness.run(&mut supervisor, vec![RuntimeEventSource::NotifySocket]);

    let applied: Vec<&[u8]> = turn
        .turns
        .iter()
        .filter_map(|turn| match turn {
            RuntimeShutdownEventTurn::Notify {
                read: RuntimeNotifyRead::Datagram(read),
                supervisor: Some(RuntimeNotifySupervisorTurn::Applied(_)),
                ..
            } => Some(read.payload.as_slice()),
            _ => None,
        })
        .collect();
    assert_eq!(
        applied,
        vec![&b"STATUS=first"[..], &b"STATUS=second\nREADY=1"[..]]
    );
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Active);
    assert_eq!(status.status_text.as_deref(), Some("second"));
}

#[test]
fn the_hold_is_bounded_for_each_sender() {
    let mut held = RuntimeHeldNotifyDatagrams::default();
    for _ in 0..RuntimeHeldNotifyDatagrams::MAX_PER_SENDER {
        held.hold(datagram(APP_PID, b"READY=1"))
            .expect("within the bound");
    }
    assert!(held.hold(datagram(APP_PID, b"READY=1")).is_err());
    held.hold(datagram(APP_PID + 1, b"READY=1"))
        .expect("another sender has a bound of its own");
    assert_eq!(held.release(|pid| pid == APP_PID).len(), 1);
}

/// Boot a `Readiness=Notify` service and launch it into a pending setup, so
/// that its job is `Created` with its PID live and unrecorded.
fn app_in_pending_setup() -> (Supervisor, JobId) {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let job_id = supervisor.pending_launch_jobs()[0];
    let mut pending = process(APP_PID, APP_PIDFD);
    pending.setup_status_fd = Some(SETUP_FD);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![pending]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);
    let launch = supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("pending launch dispatch");
    assert!(matches!(
        launch,
        SupervisorServiceLaunchDispatch::PendingSetup(_)
    ));
    assert_eq!(
        supervisor.jobs().get(job_id).expect("job").state,
        JobState::Created,
        "the job must not yet carry a pid: that is the premise",
    );
    (supervisor, job_id)
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

/// The runtime's sources, kept across batches as the runtime keeps them.
struct Harness {
    notify: FakeNotifySource,
    launcher: TestProcessLauncher,
    log_pipes: crate::runtime::RuntimeServiceLogPipes,
    controller: TestProcessController,
    registrar: FakeRegistrar,
}

impl Harness {
    fn new(setup_statuses: impl IntoIterator<Item = ProcessSetupStatus>) -> Self {
        Self {
            notify: FakeNotifySource::empty(),
            launcher: TestProcessLauncher::new(Vec::new()).setup_statuses(SETUP_FD, setup_statuses),
            log_pipes: crate::runtime::RuntimeServiceLogPipes::default(),
            controller: TestProcessController::default(),
            registrar: FakeRegistrar::default(),
        }
    }

    /// Queue a datagram for the next notify-socket readiness.
    fn notify(&mut self, datagram: NotifyDatagram) {
        let held = std::mem::take(&mut self.notify.held);
        self.notify = FakeNotifySource::new([Ok(Some(datagram))]);
        self.notify.held = held;
    }

    fn run(
        &mut self,
        supervisor: &mut Supervisor,
        sources: Vec<RuntimeEventSource>,
    ) -> RuntimeShutdownLoopTurn {
        let mut registry = StaticRegistry::services(Vec::new());
        let mut signal = FakeSignalSource::would_block();
        let mut child_reaper = FakeChildReaper::empty();
        let mut listener = FakeControlListener::default();
        let mut connections = ControlConnectionTable::new(4);
        let mut deadline_timer = FakeDeadlineTimer::would_block();
        let mut lifecycle_timer = FakeDeadlineTimer::would_block();
        let mut power_button = super::support::FakePowerButtonSource::would_block();
        let mut filesystem_check_reader =
            crate::supervisor::tests::TestFilesystemCheckReader::default();
        let mut clock = ScriptedClock::new([TURN_NS; 32]);
        let mut finalizer = RuntimeFinalizer::default();
        let mut access = AllowAccessChecker::default();
        let mut tokens = TestTokenProvider::default();
        let mut filesystem_check_launcher =
            crate::supervisor::tests::TestFilesystemCheckLauncher::default();
        let mut boot_attempt_counter = FakeBootAttemptCounter::default();
        let mut jobs_channel = crate::runtime::NoJobsChannel;
        let mut job_identity_provider = crate::runtime::NoJobIdentityProvider;

        process_runtime_shutdown_sources_with_registry(
            supervisor,
            sources,
            &mut RuntimeShutdownEventSources {
                signal_source: &mut signal,
                child_reaper: &mut child_reaper,
                notify_source: &mut self.notify,
                control_listener: &mut listener,
                control_connections: &mut connections,
                deadline_timer: &mut deadline_timer,
                lifecycle_timer: &mut lifecycle_timer,
                power_button_source: &mut power_button,
                filesystem_check_reader: &mut filesystem_check_reader,
                log_pipes: &mut self.log_pipes,
                jobs_channel: &mut jobs_channel,
            },
            Some(&mut registry),
            None,
            RuntimeShutdownLoopContext {
                clock: &mut clock,
                controller: &mut self.controller,
                finalizer: &mut finalizer,
                access_checker: &mut access,
                registrar: &mut self.registrar,
                token_provider: &mut tokens,
                process_launcher: &mut self.launcher,
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
        .expect("runtime sources")
    }
}
