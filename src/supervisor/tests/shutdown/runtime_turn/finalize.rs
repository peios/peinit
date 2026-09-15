//! The final action a runtime turn ends in: taken after the turn's events,
//! announced before it is taken, and armed for retry when it returns.

use crate::boundary::{BoundaryError, ChildExitStatus, ChildReap, ShutdownFinalizer};
use crate::runtime::{
    RuntimePendingShutdownFinalization, finalize_due_shutdown, pending_shutdown_finalization,
};
use crate::service::ErrorControl;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::shutdown::{ShutdownDeadlineKind, ShutdownFinalizationState, ShutdownKind};
use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};
use crate::supervisor::{
    CriticalRebootOwed, CriticalRebootTrigger, Supervisor, SupervisorSettings,
    SupervisorShutdownDeadlineTimerTurn,
};

use super::super::SHUTDOWN_NS;
use super::super::fixture::{drive_shutdown_to_ready, shutdown_fixture};
use super::support::{DeadlineTimerCall, FakeBootAttemptCounter, FakeDeadlineTimer};

const APP_CRASH_NS: u64 = APP_LAUNCH_NS + 1_000;

/// A Critical service whose budget was exhausted by a startup failure, so
/// no inline path rebooted for it and the reboot is owed to the
/// reconciliation pass.
fn supervisor_owing_a_critical_reboot() -> (Supervisor, u64) {
    let mut app = crate::service::ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.error_control = ErrorControl::Critical;
    app.restart_max_retries = 0;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(5000, 20)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    let due_at_ns = supervisor
        .next_readiness_timeout()
        .expect("readiness timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();
    let mut counter = FakeBootAttemptCounter::default();
    supervisor
        .process_due_lifecycle_deadlines_with_finalizer(
            &mut controller,
            &mut counter,
            None,
            due_at_ns,
        )
        .expect("lifecycle deadlines")
        .expect("a readiness timeout was due");
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Failed);
    assert_eq!(status.cause, Some(TransitionCause::RestartBudgetExhausted));
    assert!(supervisor.shutdown().is_none(), "no inline path rebooted");
    (supervisor, due_at_ns)
}

// PEI-1087. Every other path that finalises re-syncs the shutdown deadline
// timer afterwards; this one did not, so when reboot(2) returned the Failed
// state's next_retry_at_ns was never armed and nothing retried.
#[test]
fn a_critical_budget_reboot_that_returns_arms_its_retry() {
    let (mut supervisor, now_ns) = supervisor_owing_a_critical_reboot();
    let mut finalizer = FailingRebootFinalizer::default();
    let mut deadline_timer = FakeDeadlineTimer::would_block();

    let turn = finalize_due_shutdown(&mut supervisor, &mut finalizer, &mut deadline_timer, now_ns)
        .expect("critical budget reboot")
        .expect("a Critical service exhausted its budget");

    let reboot = turn
        .critical_budget_reboot
        .expect("the Critical service's reboot");
    assert_eq!(reboot.service, "app");
    assert_eq!(
        reboot.trigger,
        CriticalRebootTrigger::RestartBudgetExhausted,
        "a startup failure has nothing to add to the budget line",
    );
    let next_retry_at_ns = now_ns + 1_000_000_000;
    assert_eq!(
        reboot.finalization.finalization,
        ShutdownFinalizationState::Failed {
            message: "Shutdown(\"reboot returned\")".to_string(),
            next_retry_at_ns,
        },
    );
    assert!(turn.finalization.is_none());
    assert_eq!(
        turn.deadline_timer,
        SupervisorShutdownDeadlineTimerTurn::Armed {
            deadline: crate::shutdown::ShutdownDeadline {
                due_at_ns: next_retry_at_ns,
                kind: ShutdownDeadlineKind::FinalActionRetry,
            },
        },
    );
    assert_eq!(
        deadline_timer.calls,
        vec![DeadlineTimerCall::Arm(next_retry_at_ns)],
    );
    assert_eq!(finalizer.reboots, 1);
}

#[test]
fn a_turn_with_nothing_due_leaves_the_finalizer_and_deadline_timer_alone() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut finalizer = FailingRebootFinalizer::default();
    let mut deadline_timer = FakeDeadlineTimer::would_block();

    assert_eq!(pending_shutdown_finalization(&supervisor, BOOT_NS), None);
    let turn = finalize_due_shutdown(
        &mut supervisor,
        &mut finalizer,
        &mut deadline_timer,
        BOOT_NS,
    )
    .expect("nothing due");

    assert_eq!(turn, None);
    assert!(deadline_timer.calls.is_empty());
    assert_eq!(finalizer.reboots, 0);
}

// PEI-827. A Critical service crashing out of its budget used to reboot from
// inside the reap that observed it, so the turn's console output — the
// service's failure, and the "critical service X failed" line naming it —
// was assembled after reboot(2) had already not returned. The reap now
// notes what it saw and leaves the reboot to the end of the turn, which can
// be announced first; and the audit event still names the trigger.
#[test]
fn a_critical_crash_reaped_without_a_finalizer_is_announced_and_rebooted_at_the_end_of_the_turn() {
    let mut app = alive_service("app");
    app.error_control = ErrorControl::Critical;
    app.restart_max_retries = 0;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(5000, 20)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    let mut controller = TestProcessController::default();

    let reap = supervisor
        .apply_reaped_child(
            ChildReap {
                pid: 5000,
                status: ChildExitStatus::Signaled {
                    signal: libc::SIGSEGV,
                    core_dumped: false,
                },
            },
            APP_CRASH_NS,
            &mut controller,
            None,
        )
        .expect("reap the crash");

    let crate::supervisor::SupervisorChildReapTurn::Tracked {
        dispatch: crate::supervisor::SupervisorChildReapDispatch::Runtime(terminal),
        ..
    } = reap
    else {
        panic!("expected the crash to reach the runtime terminal path");
    };
    assert!(
        terminal.critical_reboot.is_none(),
        "the reap must not have rebooted"
    );
    assert!(supervisor.shutdown().is_none());
    assert_eq!(
        pending_shutdown_finalization(&supervisor, APP_CRASH_NS + 1),
        Some(RuntimePendingShutdownFinalization::CriticalBudgetReboot(
            CriticalRebootOwed {
                service: "app".to_string(),
                trigger: CriticalRebootTrigger::ServiceMainTerminal,
            },
        )),
        "the turn can say what is about to happen, and why",
    );

    let mut finalizer = FailingRebootFinalizer::default();
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let turn = finalize_due_shutdown(
        &mut supervisor,
        &mut finalizer,
        &mut deadline_timer,
        APP_CRASH_NS + 1,
    )
    .expect("finalize")
    .expect("the reboot is taken at the end of the turn");

    let reboot = turn
        .critical_budget_reboot
        .expect("the Critical service's reboot");
    assert_eq!(reboot.service, "app");
    assert_eq!(reboot.trigger, CriticalRebootTrigger::ServiceMainTerminal);
    assert_eq!(
        reboot.observed_at_ns,
        Some(APP_CRASH_NS),
        "the audit event carries when the crash was seen, not when the reboot ran",
    );
    assert_eq!(finalizer.reboots, 1);
    assert!(
        supervisor.shutdown().is_some(),
        "and the machine is in its failed-shutdown state"
    );
    assert_eq!(
        pending_shutdown_finalization(&supervisor, APP_CRASH_NS + 2),
        None,
        "nothing is owed a second time before the retry is due",
    );
}

// PEI-827. The third SIGINT used to kill and reboot inside the signal read,
// before "shutdown forced reboot requested" could be written. Without a
// finalizer the kills are done and the reboot is installed as due, for the
// end of the turn.
#[test]
fn a_forced_reboot_without_a_finalizer_is_taken_at_the_end_of_the_turn_as_sync_and_reboot() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();

    let forced = supervisor
        .force_reboot_shutdown(&mut controller, None, SHUTDOWN_NS)
        .expect("forced shutdown");

    assert!(forced.finalization.is_none(), "not attempted yet");
    assert_eq!(forced.killed_services.len(), 4);
    assert_eq!(
        pending_shutdown_finalization(&supervisor, SHUTDOWN_NS),
        Some(RuntimePendingShutdownFinalization::FinalAction),
        "the immediate action is due at once",
    );

    let mut finalizer = ImmediateOnlyFinalizer::default();
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let turn = finalize_due_shutdown(
        &mut supervisor,
        &mut finalizer,
        &mut deadline_timer,
        SHUTDOWN_NS,
    )
    .expect("finalize")
    .expect("the forced reboot is taken at the end of the turn");

    assert!(turn.critical_budget_reboot.is_none());
    assert_eq!(
        turn.finalization.expect("finalization").finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(finalizer.calls, vec!["sync", "reboot"]);
    assert_eq!(
        turn.deadline_timer,
        SupervisorShutdownDeadlineTimerTurn::Disarmed
    );
}

// PEI-827. A graceful shutdown's final action likewise waits for the end of
// the turn: the reap that made it Ready no longer finalises.
#[test]
fn a_ready_shutdown_driven_without_a_finalizer_is_finalized_at_the_end_of_the_turn() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    drive_shutdown_to_ready(&mut supervisor, ShutdownKind::Poweroff, &mut controller);
    assert_eq!(
        pending_shutdown_finalization(&supervisor, SHUTDOWN_NS + 4),
        Some(RuntimePendingShutdownFinalization::FinalAction),
    );

    let drive = supervisor
        .drive_shutdown(&mut controller, None, SHUTDOWN_NS + 4)
        .expect("drive shutdown");
    assert!(
        drive.is_none(),
        "no timeouts were due and the final action was left alone"
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").finalization,
        ShutdownFinalizationState::Ready,
    );

    let mut finalizer = ImmediateOnlyFinalizer::default().allowing_mount_cleanup();
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let turn = finalize_due_shutdown(
        &mut supervisor,
        &mut finalizer,
        &mut deadline_timer,
        SHUTDOWN_NS + 4,
    )
    .expect("finalize")
    .expect("the final action is taken at the end of the turn");

    assert_eq!(
        turn.finalization.expect("finalization").finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(
        finalizer.calls,
        vec!["snapshot", "remount /", "sync", "reboot"],
    );
}

#[derive(Debug, Default)]
struct FailingRebootFinalizer {
    reboots: usize,
}

impl ShutdownFinalizer for FailingRebootFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        panic!("critical reboot must not snapshot mounts");
    }

    fn unmount(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        panic!("critical reboot must not unmount");
    }

    fn remount_readonly(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        panic!("critical reboot must not remount");
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        assert_eq!(kind, ShutdownKind::Reboot);
        self.reboots += 1;
        Err(BoundaryError::Shutdown("reboot returned".to_string()))
    }
}

/// Records its calls; refuses the mount cleanup unless told otherwise, so an
/// immediate action that strayed into the graceful sequence is caught.
#[derive(Debug, Default)]
struct ImmediateOnlyFinalizer {
    calls: Vec<String>,
    mount_cleanup_allowed: bool,
}

impl ImmediateOnlyFinalizer {
    fn allowing_mount_cleanup(mut self) -> Self {
        self.mount_cleanup_allowed = true;
        self
    }
}

impl ShutdownFinalizer for ImmediateOnlyFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        assert!(self.mount_cleanup_allowed, "must not snapshot mounts");
        self.calls.push("snapshot".to_string());
        Ok(vec!["/".to_string()])
    }

    fn unmount(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        assert!(self.mount_cleanup_allowed, "must not unmount");
        self.calls.push(format!("unmount {mount_point}"));
        Ok(())
    }

    fn remount_readonly(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        assert!(self.mount_cleanup_allowed, "must not remount");
        self.calls.push(format!("remount {mount_point}"));
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push("sync".to_string());
        Ok(())
    }

    fn reboot(&mut self, _kind: ShutdownKind) -> Result<(), BoundaryError> {
        self.calls.push("reboot".to_string());
        Ok(())
    }
}
