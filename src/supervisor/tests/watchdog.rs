use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::service::ErrorControl;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::shutdown::ShutdownKind;
use crate::supervisor::{
    Supervisor, SupervisorSettings, SupervisorWatchdogNotifyOutcome,
    SupervisorWatchdogTimeoutOutcome,
};

use super::{
    APP_LAUNCH_NS, BOOT_NS, RESTART_LAUNCH_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, alive_service, process,
    settings,
};

const WATCHDOG_TIMEOUT_SECS: u64 = 5;
const WATCHDOG_TIMEOUT_NS: u64 = WATCHDOG_TIMEOUT_SECS * 1_000_000_000;

#[test]
fn active_simple_service_with_schema_watchdog_arms_timeout() {
    let supervisor = active_watchdog_supervisor();

    let deadline = supervisor
        .next_watchdog_timeout()
        .expect("watchdog timeout");

    assert_eq!(deadline.service, "app");
    assert_eq!(deadline.generation, 1);
    assert_eq!(deadline.due_at_ns, APP_LAUNCH_NS + WATCHDOG_TIMEOUT_NS);
}

#[test]
fn watchdog_keepalive_rearms_current_generation() {
    let mut supervisor = active_watchdog_supervisor();
    let mut controller = TestProcessController::default();
    let keepalive_at_ns = APP_LAUNCH_NS + 100_000;

    let dispatch = supervisor
        .apply_notify_datagram(
            datagram(8000, b"WATCHDOG=1"),
            keepalive_at_ns,
            &mut controller,
        )
        .expect("watchdog keepalive")
        .into_service()
        .expect("service notify outcome");

    assert_eq!(dispatch.watchdog_notifications.len(), 1);
    assert_eq!(
        dispatch.watchdog_notifications[0].outcome,
        SupervisorWatchdogNotifyOutcome::Armed {
            due_at_ns: keepalive_at_ns + WATCHDOG_TIMEOUT_NS,
        },
    );
    assert_eq!(
        supervisor
            .next_watchdog_timeout()
            .expect("watchdog timeout")
            .due_at_ns,
        keepalive_at_ns + WATCHDOG_TIMEOUT_NS,
    );
}

#[test]
fn watchdog_usec_runtime_update_rearms_and_zero_disables() {
    let mut supervisor = active_watchdog_supervisor();
    let mut controller = TestProcessController::default();
    let update_at_ns = APP_LAUNCH_NS + 200_000;

    let update = supervisor
        .apply_notify_datagram(
            datagram(8000, b"WATCHDOG_USEC=2000"),
            update_at_ns,
            &mut controller,
        )
        .expect("watchdog update")
        .into_service()
        .expect("service notify outcome");

    assert_eq!(
        update.watchdog_notifications[0].outcome,
        SupervisorWatchdogNotifyOutcome::Armed {
            due_at_ns: update_at_ns + 2_000_000,
        },
    );
    assert_eq!(
        supervisor
            .next_watchdog_timeout()
            .expect("watchdog timeout")
            .due_at_ns,
        update_at_ns + 2_000_000,
    );

    let disable = supervisor
        .apply_notify_datagram(
            datagram(8000, b"WATCHDOG_USEC=0"),
            update_at_ns + 1_000,
            &mut controller,
        )
        .expect("watchdog disable")
        .into_service()
        .expect("service notify outcome");

    assert_eq!(
        disable.watchdog_notifications[0].outcome,
        SupervisorWatchdogNotifyOutcome::Disabled,
    );
    assert!(supervisor.next_watchdog_timeout().is_none());
}

#[test]
fn watchdog_timeout_kills_service_cgroup_and_enters_restart_backoff() {
    let mut supervisor = active_watchdog_supervisor();
    let timeout = supervisor
        .next_watchdog_timeout()
        .expect("watchdog timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();

    let dispatches = supervisor
        .process_due_watchdog_timeouts(&mut controller, timeout)
        .expect("watchdog timeout");

    assert_eq!(dispatches.len(), 1);
    assert_eq!(
        dispatches[0].outcome,
        SupervisorWatchdogTimeoutOutcome::RestartScheduled,
    );
    assert_eq!(
        dispatches[0].service_transitions[0].event.cause,
        TransitionCause::WatchdogTimeout,
    );
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Backoff);
    assert!(status.current_job.is_none());
    assert!(supervisor.next_watchdog_timeout().is_none());
    assert_eq!(
        supervisor
            .next_restart_backoff_deadline()
            .expect("restart backoff")
            .service,
        "app",
    );
}

#[test]
fn watchdog_timeout_critical_budget_exhaustion_reboots_with_finalizer() {
    let mut app = watchdog_service();
    app.error_control = ErrorControl::Critical;
    app.restart_max_retries = 0;
    let mut supervisor = active_supervisor(app, 8000, 50);
    let timeout = supervisor
        .next_watchdog_timeout()
        .expect("watchdog timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();
    let mut finalizer = RecordingFinalizer::default();

    let dispatches = supervisor
        .process_due_watchdog_timeouts_with_finalizer(
            &mut controller,
            Some(&mut finalizer),
            timeout,
        )
        .expect("watchdog timeout");

    assert_eq!(dispatches.len(), 1);
    assert_eq!(
        dispatches[0].outcome,
        SupervisorWatchdogTimeoutOutcome::Failed
    );
    assert_eq!(
        dispatches[0].service_transitions[0].event.cause,
        TransitionCause::RestartBudgetExhausted,
    );
    assert!(dispatches[0].critical_reboot.is_some());
    assert_eq!(
        finalizer.calls,
        vec![FinalizerCall::Sync, FinalizerCall::Reboot],
    );
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn runtime_watchdog_update_does_not_persist_across_restart() {
    let mut supervisor = active_watchdog_supervisor();
    let mut controller = TestProcessController::default();
    let update_at_ns = APP_LAUNCH_NS + 200_000;
    supervisor
        .apply_notify_datagram(
            datagram(8000, b"WATCHDOG_USEC=2000"),
            update_at_ns,
            &mut controller,
        )
        .expect("watchdog update")
        .into_service()
        .expect("service notify outcome");
    supervisor
        .process_due_watchdog_timeouts(&mut controller, update_at_ns + 2_000_000)
        .expect("watchdog timeout");
    let restart_due = supervisor
        .next_restart_backoff_deadline()
        .expect("restart backoff")
        .due_at_ns;
    supervisor
        .process_due_restart_backoffs(restart_due)
        .expect("restart backoff");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8001, 51)]);
    let mut clock = ScriptedClock::new([RESTART_LAUNCH_NS]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch restart")
        .expect("restart launch");

    let deadline = supervisor
        .next_watchdog_timeout()
        .expect("schema watchdog timeout");
    assert_eq!(deadline.generation, 2);
    assert_eq!(deadline.due_at_ns, RESTART_LAUNCH_NS + WATCHDOG_TIMEOUT_NS);
}

fn active_watchdog_supervisor() -> Supervisor {
    active_supervisor(watchdog_service(), 8000, 50)
}

fn watchdog_service() -> crate::service::ServiceDefinition {
    let mut app = alive_service("app");
    app.watchdog_timeout_secs = WATCHDOG_TIMEOUT_SECS;
    app
}

fn active_supervisor(app: crate::service::ServiceDefinition, pid: u32, pidfd: i32) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, pidfd)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");
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

#[derive(Debug, Default, PartialEq, Eq)]
struct RecordingFinalizer {
    calls: Vec<FinalizerCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FinalizerCall {
    Sync,
    Reboot,
}

impl ShutdownFinalizer for RecordingFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        Ok(Vec::new())
    }

    fn unmount(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn remount_readonly(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(FinalizerCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        assert_eq!(kind, ShutdownKind::Reboot);
        self.calls.push(FinalizerCall::Reboot);
        Ok(())
    }
}
