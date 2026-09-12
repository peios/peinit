use crate::boundary::ProcessSignal;
use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::operation::OperationSource;
use crate::service::RestartPolicy;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    APP_CRASH_NS, APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, alive_service, process,
    settings,
};

const CONTROL_NS: u64 = 1_700_000_000;
const STOPPED_NS: u64 = 1_700_010_000;
const RECOVERY_NS: u64 = 1_800_000_000;
const RECOVERY_LAUNCH_NS: u64 = 1_800_010_000;

#[test]
fn conflict_start_stops_active_reverse_declared_conflict_before_winner_launches() {
    let mut app = alive_service("app");
    app.triggers.clear();
    let mut incumbent = alive_service("incumbent");
    incumbent.conflicts.push("app".to_string());
    let mut supervisor = boot_and_launch(vec![app, incumbent], [BOOT_NS, APP_LAUNCH_NS]);

    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let start = supervisor
        .start_service("app", None, &mut clock)
        .expect("start app");
    let LifecycleCommandOutcome::OnDemandStart(dispatch) = &start.outcome else {
        panic!("expected on-demand start");
    };
    assert!(start.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(
        supervisor.pending_control_operations()[0].service,
        "incumbent",
    );
    assert_eq!(
        supervisor
            .operation_status(supervisor.pending_control_operations()[0].operation_id)
            .expect("conflict stop operation")
            .source,
        OperationSource::ConflictResolution,
    );
    assert_eq!(
        dispatch.requested_operation.returned_operation_id,
        supervisor
            .service_status("app")
            .expect("app status")
            .current_operation
            .expect("app operation")
            .id,
    );

    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute conflict stop")
        .expect("conflict stop dispatch");
    assert_eq!(controller.signals[0].signal, ProcessSignal::Sigterm);
    assert_eq!(
        supervisor
            .service_status("incumbent")
            .expect("incumbent")
            .cause,
        Some(TransitionCause::ConflictEviction),
    );

    let incumbent_job = current_job(&supervisor, "incumbent");
    let terminal = supervisor
        .complete_job(incumbent_job, STOPPED_NS, 0)
        .expect("incumbent stopped");
    assert_eq!(
        terminal
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    assert_eq!(
        supervisor
            .service_status("incumbent")
            .expect("incumbent")
            .state,
        ServiceState::Failed,
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
}

/// §6.1 and §7.1: Reloading satisfies dependents, so reloading a `BindsTo`
/// target is not the target going away. Before PEI-1079 the move into
/// Reloading read as losing satisfaction and queued a `BindsToPropagation`
/// stop of every bound dependent, and the return to Active then "recovered"
/// them.
#[test]
fn reloading_a_binds_to_target_leaves_its_dependent_alone() {
    let mut app = alive_service("app");
    app.binds_to.push("db".to_string());
    let mut db = alive_service("db");
    db.exec_reload = Some("/bin/reload".to_string());
    let mut supervisor =
        boot_and_launch(vec![app, db], [BOOT_NS, APP_LAUNCH_NS, APP_LAUNCH_NS + 1]);

    let mut controller = TestProcessController::default();
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9200, 93)]);
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        CONTROL_NS,
        CONTROL_NS + 1,
        CONTROL_NS + 2,
    ]);
    supervisor
        .reload_service("db", None, &mut clock)
        .expect("reload db");
    let execution = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload execution");
    let crate::execution::control::ControlExecutionDetail::ReloadCommand { job_id, .. } =
        execution.execution.detail
    else {
        panic!("expected a reload command");
    };
    supervisor
        .launch_next_pending_control_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch reload command")
        .expect("reload command launch");
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Reloading,
    );
    assert!(
        supervisor.pending_control_operations().is_empty(),
        "no stop was queued for the bound dependent",
    );
    assert!(controller.signals.is_empty(), "and nothing was signalled");

    supervisor
        .complete_reload_command_job(job_id, STOPPED_NS, 0, &mut controller)
        .expect("reload command succeeds");
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Active,
    );
    let app = supervisor.service_status("app").expect("app");
    assert_eq!(app.state, ServiceState::Active);
    assert_ne!(app.cause, Some(TransitionCause::BindsToPropagation));
    assert!(supervisor.pending_control_operations().is_empty());
    assert!(
        supervisor.pending_launch_jobs().is_empty(),
        "and nothing was started to recover it",
    );
}

#[test]
fn binds_to_stop_propagates_and_recovery_restarts_failed_dependent() {
    let mut app = alive_service("app");
    app.binds_to.push("db".to_string());
    let db = alive_service("db");
    let mut supervisor =
        boot_and_launch(vec![app, db], [BOOT_NS, APP_LAUNCH_NS, APP_LAUNCH_NS + 1]);

    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    supervisor
        .stop_service("db", None, &mut clock)
        .expect("stop db");
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS, CONTROL_NS + 1]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute db stop")
        .expect("db stop dispatch");
    assert_eq!(supervisor.pending_control_operations()[0].service, "app",);

    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute app binds stop")
        .expect("app stop dispatch");
    assert_eq!(
        supervisor.service_status("app").expect("app").cause,
        Some(TransitionCause::BindsToPropagation),
    );

    let app_job = current_job(&supervisor, "app");
    supervisor
        .complete_job(app_job, STOPPED_NS, 0)
        .expect("app stopped by binds");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Failed,
    );
    let db_job = current_job(&supervisor, "db");
    supervisor
        .complete_job(db_job, STOPPED_NS + 1, 0)
        .expect("db stopped");

    let mut clock = ScriptedClock::new([RECOVERY_NS, RECOVERY_LAUNCH_NS]);
    let db_start = supervisor
        .start_service("db", None, &mut clock)
        .expect("restart db");
    assert_eq!(
        db_start
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["db"],
    );

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9100, 91), process(9101, 92)]);
    let db_launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch db recovery")
        .expect("db launch");
    assert_eq!(
        db_launch
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    let app_operation = supervisor
        .service_status("app")
        .expect("app status")
        .current_operation
        .expect("app recovery operation")
        .id;
    assert_eq!(
        supervisor
            .operation_status(app_operation)
            .expect("app recovery operation")
            .source,
        OperationSource::BindsToRecovery,
    );
}

#[test]
fn on_failure_starts_fallback_when_service_enters_failed() {
    let mut app = alive_service("app");
    app.on_failure = Some("fallback".to_string());
    app.restart_policy = RestartPolicy::Never;
    let mut fallback = alive_service("fallback");
    fallback.triggers.clear();
    let mut supervisor = boot_and_launch(vec![app, fallback], [BOOT_NS, APP_LAUNCH_NS]);
    let app_job = current_job(&supervisor, "app");

    let terminal = supervisor
        .complete_job(app_job, APP_CRASH_NS, 1)
        .expect("app failed");

    assert_eq!(
        terminal
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["fallback"],
    );
    let fallback_operation = supervisor
        .service_status("fallback")
        .expect("fallback")
        .current_operation
        .expect("fallback operation")
        .id;
    assert_eq!(
        supervisor
            .operation_status(fallback_operation)
            .expect("fallback operation")
            .source,
        OperationSource::OnFailure,
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
}

#[test]
fn on_failure_self_reference_is_suppressed_at_runtime() {
    let mut app = alive_service("app");
    app.on_failure = Some("app".to_string());
    app.restart_policy = RestartPolicy::Never;
    let mut supervisor = boot_and_launch(vec![app], [BOOT_NS, APP_LAUNCH_NS]);
    let app_job = current_job(&supervisor, "app");

    let terminal = supervisor
        .complete_job(app_job, APP_CRASH_NS, 1)
        .expect("app failed");

    assert!(terminal.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
}

// PEI-597. A relationship start never passes through `admit_lifecycle_command`,
// so it never got that path's "already active -> no-op" answer. An OnFailure
// handler that was already Active was therefore planned for a start it has no
// legal transition into ((Active, Starting) has no arm in the rule table), and
// the InvalidTransition surfaced out of the failure-reaction pipeline rather
// than as a benign refusal.
//
// The loop guard used to mask this: a handler that started and stayed running
// kept its chain-membership entry forever, so a second attempt was refused as a
// cycle before it reached the transition. PEI-362 retires that entry once the
// handler has been dependent-satisfying for its RestartWindow, which leaves the
// path reachable for a *fresh* failure whose handler is already up. This test
// takes the case the guard never covered at all: the handler is Active because
// it was boot-triggered, not because it was started by an earlier failure, so
// no chain exists to save it.
#[test]
fn an_on_failure_handler_that_is_already_active_is_refused_benignly() {
    let mut app = alive_service("app");
    app.on_failure = Some("fallback".to_string());
    app.restart_policy = RestartPolicy::Never;
    // Triggers left intact, so the handler is Active before anything fails.
    let fallback = alive_service("fallback");
    // Three ticks: the boot itself, then one per boot-triggered launch.
    let mut supervisor = boot_and_launch(
        vec![app, fallback],
        [BOOT_NS, APP_LAUNCH_NS, APP_LAUNCH_NS + 1],
    );

    let before = supervisor.service_status("fallback").expect("fallback");
    assert_eq!(before.state, ServiceState::Active);
    let handler_job = before.current_job.expect("handler job").id;

    let app_job = current_job(&supervisor, "app");
    let terminal = supervisor
        .complete_job(app_job, APP_CRASH_NS, 1)
        .expect("the failure reaction must not raise InvalidTransition");

    assert!(terminal.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());

    // The running handler is left exactly as it was — same state, same job, and
    // no operation was minted for a start that never happened.
    let after = supervisor.service_status("fallback").expect("fallback");
    assert_eq!(after.state, ServiceState::Active);
    assert_eq!(after.current_job.expect("handler job").id, handler_job);
    assert!(after.current_operation.is_none());
}

#[test]
fn on_failure_loop_guard_survives_active_fallback_crashes() {
    let mut a = alive_service("a");
    a.on_failure = Some("b".to_string());
    a.restart_policy = RestartPolicy::Never;
    let mut b = alive_service("b");
    b.triggers.clear();
    b.on_failure = Some("a".to_string());
    b.restart_policy = RestartPolicy::Never;
    let mut supervisor = boot_and_launch(vec![a, b], [BOOT_NS, APP_LAUNCH_NS]);

    let a_job = current_job(&supervisor, "a");
    let a_failure = supervisor
        .complete_job(a_job, APP_CRASH_NS, 1)
        .expect("a failed");
    assert_eq!(
        a_failure
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["b"],
    );
    launch_one_pending(&mut supervisor, APP_CRASH_NS + 1, 9200, 120);

    let b_job = current_job(&supervisor, "b");
    let b_failure = supervisor
        .complete_job(b_job, APP_CRASH_NS + 2, 1)
        .expect("b failed");
    assert_eq!(
        b_failure
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["a"],
    );
    launch_one_pending(&mut supervisor, APP_CRASH_NS + 3, 9201, 121);

    let a_job = current_job(&supervisor, "a");
    let suppressed = supervisor
        .complete_job(a_job, APP_CRASH_NS + 4, 1)
        .expect("a failed again");
    assert!(suppressed.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());

    let maintenance = supervisor
        .process_due_operation_maintenance(APP_CRASH_NS + 5)
        .expect("drain relationship audit events");
    assert_eq!(maintenance.relationship_audit_events.len(), 1);
    let audit = &maintenance.relationship_audit_events[0];
    assert_eq!(audit.failed_service, "a");
    assert_eq!(audit.attempted_handler, "b");
    assert_eq!(audit.chain, ["b", "a", "b"]);
    assert_eq!(
        audit.reason,
        crate::supervisor::SupervisorOnFailureLoopSuppressionReason::Cycle
    );
}

fn boot_and_launch<const N: usize>(
    services: Vec<crate::service::ServiceDefinition>,
    times: [u64; N],
) -> Supervisor {
    let process_count = services.len();
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(services);
    let mut clock = ScriptedClock::new(times);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(
        (0..process_count)
            .map(|index| process(9000 + index as u32, 80 + index as i32))
            .collect(),
    );
    while !supervisor.pending_launch_jobs().is_empty() {
        supervisor
            .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
            .expect("launch service")
            .expect("launch dispatch");
    }
    supervisor
}

fn launch_one_pending(supervisor: &mut Supervisor, launched_at_ns: u64, pid: u32, pidfd: i32) {
    let mut clock = ScriptedClock::new([launched_at_ns]);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, pidfd)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch pending service")
        .expect("launch dispatch");
}

fn current_job(supervisor: &Supervisor, service: &str) -> crate::ids::JobId {
    supervisor
        .service_status(service)
        .expect("service status")
        .current_job
        .expect("current job")
        .id
}

// PEI-362. The chain that bounds an OnFailure cascade is recorded against the
// handler and was only cleared when that handler reached a terminal or
// inactive state — so a handler that started and *stayed running* held its
// membership entry and one of the sixteen depth slots for as long as the entry
// lived. The guard was consumed by exactly the case that worked: a degradation
// path handing off through several healthy layers exhausted its own depth
// budget, and a failover ping-pong was refused as a cycle on its second round
// even when every hop had come up healthy.
//
// What retires the entry is deliberately not `Active`. `on_failure_loop_guard_
// survives_active_fallback_crashes` above pins the other side: a handler that
// crashes straight after coming up is still caught, and it has to be —
// `Readiness=Alive` reports Active the instant the process spawns, so clearing
// there would let two mutually-handling services hand off forever with no
// delay and no cost, since an OnFailure start is not a restart and spends no
// budget. That is the crash-loop §5.2 says MUST be bounded.
//
// The line between them is peinit's existing notion of a service having
// *arrived*: dependent-satisfying for `RestartWindow`, the same window that
// resets a restart budget.
#[test]
fn a_handler_that_holds_a_window_of_health_releases_its_chain_slot() {
    let mut a = alive_service("a");
    a.on_failure = Some("b".to_string());
    a.restart_policy = RestartPolicy::Never;
    let mut b = alive_service("b");
    b.triggers.clear();
    b.on_failure = Some("a".to_string());
    b.restart_policy = RestartPolicy::Never;
    let window_ns = b.restart_window_secs * 1_000_000_000;
    let mut supervisor = boot_and_launch(vec![a, b], [BOOT_NS, APP_LAUNCH_NS]);

    // a fails, b takes over and holds up for a full window.
    let a_job = current_job(&supervisor, "a");
    supervisor
        .complete_job(a_job, APP_CRASH_NS, 1)
        .expect("a failed");
    launch_one_pending(&mut supervisor, APP_CRASH_NS + 1, 9200, 120);
    let b_settled_ns = APP_CRASH_NS + 1 + window_ns;
    supervisor
        .process_due_operation_maintenance(b_settled_ns)
        .expect("settle b's chain");

    // b fails much later, a takes over and holds up for a full window too.
    let b_job = current_job(&supervisor, "b");
    supervisor
        .complete_job(b_job, b_settled_ns + 1, 1)
        .expect("b failed");
    launch_one_pending(&mut supervisor, b_settled_ns + 2, 9201, 121);
    let a_settled_ns = b_settled_ns + 2 + window_ns;
    let maintenance = supervisor
        .process_due_operation_maintenance(a_settled_ns)
        .expect("settle a's chain");
    assert_eq!(
        maintenance
            .on_failure_chain_settles
            .iter()
            .map(|settle| settle.service.as_str())
            .collect::<Vec<_>>(),
        vec!["a"],
    );

    // The third hop. Each of the two before it arrived, so this is a new
    // originating failure rather than a continuation, and b must be startable.
    let a_job = current_job(&supervisor, "a");
    let third_hop = supervisor
        .complete_job(a_job, a_settled_ns + 1, 1)
        .expect("a failed again");

    assert_eq!(
        third_hop
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["b"],
        "a hand-off between two handlers that each held a window of health was \
         suppressed as a loop",
    );
    let audit = supervisor
        .process_due_operation_maintenance(a_settled_ns + 2)
        .expect("drain relationship audit events");
    assert!(audit.relationship_audit_events.is_empty());
}
