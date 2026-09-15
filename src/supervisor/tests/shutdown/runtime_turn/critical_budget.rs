//! The Critical-budget reboot the runtime raises once per turn, and what it
//! does when `reboot(2)` returns.

use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::runtime::process_due_critical_budget_reboot;
use crate::service::ErrorControl;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::shutdown::{ShutdownDeadlineKind, ShutdownFinalizationState, ShutdownKind};
use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, process, settings,
};
use crate::supervisor::{Supervisor, SupervisorSettings, SupervisorShutdownDeadlineTimerTurn};

use super::support::{DeadlineTimerCall, FakeBootAttemptCounter, FakeDeadlineTimer};

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

    let turn = process_due_critical_budget_reboot(
        &mut supervisor,
        &mut finalizer,
        &mut deadline_timer,
        now_ns,
    )
    .expect("critical budget reboot")
    .expect("a Critical service exhausted its budget");

    assert_eq!(turn.dispatch.service, "app");
    let next_retry_at_ns = now_ns + 1_000_000_000;
    assert_eq!(
        turn.dispatch.finalization.finalization,
        ShutdownFinalizationState::Failed {
            message: "Shutdown(\"reboot returned\")".to_string(),
            next_retry_at_ns,
        },
    );
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
fn a_turn_with_no_critical_reboot_owed_leaves_the_deadline_timer_alone() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut finalizer = FailingRebootFinalizer::default();
    let mut deadline_timer = FakeDeadlineTimer::would_block();

    let turn = process_due_critical_budget_reboot(
        &mut supervisor,
        &mut finalizer,
        &mut deadline_timer,
        BOOT_NS,
    )
    .expect("no reboot owed");

    assert_eq!(turn, None);
    assert!(deadline_timer.calls.is_empty());
    assert_eq!(finalizer.reboots, 0);
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
