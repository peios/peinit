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

// PEI-341. Two rules that are individually right combining into a state
// neither intended.
//
// §5.3: a Critical service that exhausts its restart budget syncs and reboots,
// and the reboot takes precedence over `OnFailure`. §5.2: peinit must not
// start the `OnFailure` service in that case. Both assume the reboot happens.
//
// The reboot was raised only from the paths that observe a terminal outcome
// for a *running* service — the main job ending, health checks, the watchdog.
// A budget exhausted by startup failures (repeated ReadinessTimeout,
// PreHookFailure, ParentSetupFailure) reached none of them, while the
// suppression keyed on the cause and ErrorControl alone and fired anyway. So a
// Critical service that could never get as far as running settled quietly in
// Failed with no reboot and no handler — the case that most needs one of the
// two escalations got neither.
#[test]
fn a_critical_budget_exhausted_by_a_startup_failure_still_reboots() {
    // Notify readiness, so failing to signal ready is the failure. Budget of
    // zero: the first failure exhausts it.
    let mut app = crate::service::ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.error_control = ErrorControl::Critical;
    app.restart_max_retries = 0;
    app.on_failure = Some("fallback".to_string());
    let mut fallback = alive_service("fallback");
    fallback.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, fallback]);
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
    let mut controller = super::TestProcessController::default();
    let mut counter = NoBootAttemptCounter;
    let mut finalizer = CriticalFinalizer::default();
    let drive = supervisor
        .process_due_lifecycle_deadlines_with_finalizer(
            &mut controller,
            &mut counter,
            Some(&mut finalizer),
            due_at_ns,
        )
        .expect("lifecycle deadlines")
        .expect("a readiness timeout was due");

    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Failed);
    assert_eq!(status.cause, Some(TransitionCause::RestartBudgetExhausted));

    // One escalation or the other must happen. Before the fix neither did:
    // the reboot was raised only by the paths that watch a running service,
    // and the handler was suppressed on the strength of it anyway.
    let handler_started = supervisor
        .service_status("fallback")
        .expect("fallback")
        .current_operation
        .is_some();
    assert!(
        handler_started || supervisor.shutdown().is_some(),
        "a Critical service exhausted its budget and got neither the reboot \
         nor its OnFailure handler",
    );

    let reboot = drive
        .critical_budget_reboot
        .expect("a Critical service exhausted its budget");
    assert_eq!(reboot.service, "app");
    assert_eq!(
        reboot.finalization.finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(
        finalizer.calls,
        vec![CriticalCall::Sync, CriticalCall::Reboot],
    );
    // The reboot takes precedence over OnFailure, so the handler stays
    // suppressed — correctly, now that the reboot actually happens.
    assert!(!handler_started);
}

// The other half of the pairing, and the reason the suppression must ask about
// the reboot rather than about ErrorControl: a Normal service gets no reboot,
// so its OnFailure handler must run.
#[test]
fn a_normal_services_exhausted_budget_starts_its_on_failure_handler() {
    let mut app = crate::service::ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.restart_max_retries = 0;
    app.on_failure = Some("fallback".to_string());
    let mut fallback = alive_service("fallback");
    fallback.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, fallback]);
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
    let mut controller = super::TestProcessController::default();
    let mut counter = NoBootAttemptCounter;
    let mut finalizer = CriticalFinalizer::default();
    let drive = supervisor
        .process_due_lifecycle_deadlines_with_finalizer(
            &mut controller,
            &mut counter,
            Some(&mut finalizer),
            due_at_ns,
        )
        .expect("lifecycle deadlines")
        .expect("a readiness timeout was due");

    assert_eq!(
        supervisor.service_status("app").expect("app").cause,
        Some(TransitionCause::RestartBudgetExhausted),
    );
    assert_eq!(
        supervisor
            .service_status("fallback")
            .expect("fallback")
            .current_operation
            .expect("fallback operation source")
            .source,
        OperationSource::OnFailure,
    );
    assert!(drive.critical_budget_reboot.is_none());
    assert!(
        finalizer.calls.is_empty(),
        "a Normal service must not reboot the machine",
    );
}

// The reconciliation pass must not fire a second time for a service the
// terminal path already rebooted for — it runs every turn, and the reboot is
// not an idempotent thing to repeat.
#[test]
fn the_reconciliation_pass_does_not_repeat_an_inline_critical_reboot() {
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
    supervisor
        .complete_job_with_shutdown_finalizer(job_id, APP_CRASH_NS, 1, &mut finalizer)
        .expect("critical crash");
    assert_eq!(
        finalizer.calls,
        vec![CriticalCall::Sync, CriticalCall::Reboot],
    );

    let mut second = CriticalFinalizer::default();
    assert!(
        supervisor
            .process_due_critical_budget_reboot(&mut second, APP_CRASH_NS + 1)
            .expect("critical budget reboot")
            .is_none(),
    );
    assert!(second.calls.is_empty());
}

/// The boot-attempt counter is not what these tests are about. The deadline
/// turn resets it when the boot succeeds, which is orthogonal to the restart
/// budget, so this accepts the reset and records nothing.
struct NoBootAttemptCounter;

impl crate::boundary::BootAttemptCounter for NoBootAttemptCounter {
    fn reset_boot_attempt_counter(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }
}

// PEI-361. §5.3's evaluate_restart: under `RestartPolicy=OnFailure`, an exit
// whose code is in `SuccessExitCodes` is not a failure and is not restarted.
// The branch was implemented exactly; the exit code just never reached it on
// the pre-readiness path, because `StartFailureRequest` had no field for one
// and the call site passed None with `ended.exit_code` in hand.
//
// So `SuccessExitCodes` quietly meant one thing after readiness and another
// before it, and a Simple service that legitimately concludes "nothing to do"
// during startup — a migration runner, a conditional setup task — restart-
// looped until its budget was exhausted, then landed in Failed with
// RestartBudgetExhausted, describing a service that had succeeded every time.
#[test]
fn a_success_exit_code_before_readiness_is_not_restarted_under_on_failure() {
    // Notify readiness, so exiting at all is a pre-readiness exit.
    let mut app = crate::service::ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.restart_policy = crate::service::RestartPolicy::OnFailure;
    app.success_exit_codes = vec![0, 7];
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
        .expect("starting app")
        .current_job
        .expect("app job")
        .id;

    // 7 is listed as success. It exits before ever signalling READY=1.
    supervisor
        .complete_job(job_id, APP_CRASH_NS, 7)
        .expect("app exits with a success code");

    let status = supervisor.service_status("app").expect("app");
    assert_eq!(
        status.state,
        ServiceState::Failed,
        "a success exit before readiness must not be restarted",
    );
    assert!(supervisor.next_restart_backoff_deadline().is_none());
    assert_eq!(
        supervisor
            .services()
            .runtime("app")
            .expect("runtime")
            .consecutive_restart_failures,
        0,
        "a success exit must not consume restart budget",
    );
}

// The control: a code that is *not* listed still restarts. The fix must not
// turn every pre-readiness exit into a terminal failure.
#[test]
fn a_failure_exit_code_before_readiness_still_restarts_under_on_failure() {
    let mut app = crate::service::ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.restart_policy = crate::service::RestartPolicy::OnFailure;
    app.success_exit_codes = vec![0, 7];
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
        .expect("starting app")
        .current_job
        .expect("app job")
        .id;

    supervisor
        .complete_job(job_id, APP_CRASH_NS, 1)
        .expect("app crashes");

    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Backoff,
    );
}
