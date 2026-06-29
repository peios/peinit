use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::operation::{OperationSource, OperationState};
use crate::service::ErrorControl;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::shutdown::{ShutdownFinalizationState, ShutdownKind};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    APP_CRASH_NS, APP_LAUNCH_NS, BOOT_NS, RESTART_LAUNCH_NS, ScriptedClock, StaticRegistry,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};

#[test]
fn active_crash_enters_backoff_then_relaunches_through_same_queue() {
    let app = alive_service("app");
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS, RESTART_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(5000, 20), process(5001, 21)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    let active = supervisor.service_status("app").expect("active app");
    let first_job = active.current_job.expect("first app job").id;

    supervisor
        .complete_job(first_job, APP_CRASH_NS, 1)
        .expect("crash app");
    let backoff = supervisor.service_status("app").expect("backoff app");
    assert_eq!(backoff.state, ServiceState::Backoff);
    assert!(backoff.current_job.is_none());
    let deadline = supervisor
        .next_restart_backoff_deadline()
        .expect("restart deadline");
    assert_eq!(deadline.service, "app");
    assert_eq!(deadline.due_at_ns, APP_CRASH_NS + 1_000_000_000);

    assert!(
        supervisor
            .process_due_restart_backoffs(deadline.due_at_ns - 1)
            .expect("early restart scan")
            .is_empty()
    );
    let relaunches = supervisor
        .process_due_restart_backoffs(deadline.due_at_ns)
        .expect("due restart scan");
    assert_eq!(relaunches.len(), 1);
    assert_eq!(
        relaunches[0]
            .relaunch
            .admission
            .requested_operation
            .returned_operation_id,
        relaunches[0].relaunch.start_dispatches[0]
            .ready
            .operation_id,
    );

    let starting = supervisor.service_status("app").expect("starting app");
    assert_eq!(starting.state, ServiceState::Starting);
    assert_eq!(starting.generation, 2);
    let restart_operation = starting.current_operation.expect("restart operation").id;
    assert_eq!(
        supervisor
            .operation_status(restart_operation)
            .expect("restart operation")
            .source,
        OperationSource::RestartPolicy,
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);

    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch restart")
        .expect("restart launch dispatch");
    let relaunched = supervisor.service_status("app").expect("relaunched app");
    assert_eq!(relaunched.state, ServiceState::Active);
    assert_eq!(relaunched.generation, 2);
    assert_eq!(
        relaunched.current_job.expect("relaunched job").pid,
        Some(5001),
    );
    assert_eq!(
        supervisor
            .operation_status(restart_operation)
            .expect("completed restart operation")
            .state,
        OperationState::Completed,
    );
}

#[test]
fn start_during_backoff_waits_for_deadline_and_uses_same_operation() {
    let app = alive_service("app");
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
    let first_job = supervisor
        .service_status("app")
        .expect("active app")
        .current_job
        .expect("first app job")
        .id;

    supervisor
        .complete_job(first_job, APP_CRASH_NS, 1)
        .expect("crash app");
    let deadline = supervisor
        .next_restart_backoff_deadline()
        .expect("restart deadline");
    let mut command_clock = ScriptedClock::new([APP_CRASH_NS + 10]);

    let start = supervisor
        .start_service("app", None, &mut command_clock)
        .expect("start during backoff");

    let LifecycleCommandOutcome::OperationAccepted(operation) = start.outcome else {
        panic!("expected deferred operation");
    };
    let deferred_operation_id = operation.returned_operation_id;
    assert!(start.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(
        supervisor
            .operation_status(deferred_operation_id)
            .expect("deferred start")
            .state,
        OperationState::Pending,
    );

    let relaunches = supervisor
        .process_due_restart_backoffs(deadline.due_at_ns)
        .expect("due restart scan");

    assert_eq!(relaunches.len(), 1);
    assert_eq!(
        relaunches[0]
            .relaunch
            .admission
            .requested_operation
            .returned_operation_id,
        deferred_operation_id,
    );
    let status = supervisor.service_status("app").expect("starting app");
    assert_eq!(status.state, ServiceState::Starting);
    assert_eq!(
        status.current_operation.expect("current operation").id,
        deferred_operation_id,
    );
    assert_eq!(
        supervisor
            .operation_status(deferred_operation_id)
            .expect("deferred start")
            .source,
        OperationSource::Admin,
    );
}

#[test]
fn active_restart_window_resets_consecutive_failure_counter() {
    let mut app = alive_service("app");
    app.restart_window_secs = 2;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS, RESTART_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(5000, 20), process(5001, 21)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    let first_job = supervisor
        .service_status("app")
        .expect("active app")
        .current_job
        .expect("first app job")
        .id;

    supervisor
        .complete_job(first_job, APP_CRASH_NS, 1)
        .expect("crash app");
    let deadline = supervisor
        .next_restart_backoff_deadline()
        .expect("restart deadline");
    supervisor
        .process_due_restart_backoffs(deadline.due_at_ns)
        .expect("due restart scan");
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch restart")
        .expect("restart launch dispatch");
    assert_eq!(
        supervisor
            .services()
            .runtime("app")
            .expect("runtime")
            .consecutive_restart_failures,
        1,
    );

    let reset_due_ns = RESTART_LAUNCH_NS + 2_000_000_000;
    let early = supervisor
        .process_due_operation_maintenance(reset_due_ns - 1)
        .expect("early maintenance");
    assert!(early.restart_window_resets.is_empty());
    assert_eq!(
        supervisor
            .services()
            .runtime("app")
            .expect("runtime")
            .consecutive_restart_failures,
        1,
    );

    let due = supervisor
        .process_due_operation_maintenance(reset_due_ns)
        .expect("due maintenance");
    assert_eq!(due.restart_window_resets.len(), 1);
    assert_eq!(due.restart_window_resets[0].service, "app");
    assert_eq!(
        supervisor
            .services()
            .runtime("app")
            .expect("runtime")
            .consecutive_restart_failures,
        0,
    );
}

#[test]
fn critical_restart_budget_exhaustion_syncs_and_reboots_immediately() {
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
    let job_id = supervisor
        .service_status("app")
        .expect("active app")
        .current_job
        .expect("app job")
        .id;
    let mut finalizer = CriticalFinalizer::default();

    let dispatch = supervisor
        .complete_job_with_shutdown_finalizer(job_id, APP_CRASH_NS, 1, &mut finalizer)
        .expect("critical crash");

    assert_eq!(
        dispatch.terminal.service_transitions[0].event.cause,
        TransitionCause::RestartBudgetExhausted,
    );
    assert_eq!(
        supervisor.service_status("app").expect("failed app").state,
        ServiceState::Failed,
    );
    assert_eq!(
        dispatch
            .critical_reboot
            .expect("critical reboot")
            .finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(
        finalizer.calls,
        vec![CriticalCall::Sync, CriticalCall::Reboot],
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Reboot,
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CriticalCall {
    Sync,
    Reboot,
}

#[derive(Debug, Default)]
struct CriticalFinalizer {
    calls: Vec<CriticalCall>,
}

impl ShutdownFinalizer for CriticalFinalizer {
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
        self.calls.push(CriticalCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        assert_eq!(kind, ShutdownKind::Reboot);
        self.calls.push(CriticalCall::Reboot);
        Ok(())
    }
}
