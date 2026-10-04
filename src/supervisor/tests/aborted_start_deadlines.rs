//! PEI-1267: a start aborted by a stop left its readiness deadline behind.
//!
//! The deadline came due against an operation that was no longer Running.
//! Acting on it raised, the raise was contained against whatever the service
//! was doing by then — the next start, which it failed — and, the operation
//! being unreachable through the active records, the deadline was never
//! cleared: it was due again on every timer turn, and every turn announced
//! the same internal error. On the dev VM that was thousands of events a
//! second into eventd until the disk filled.

use crate::boundary::BootAttemptCounter;
use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::execution::start::ReadinessDeadline;
use crate::ids::OperationId;
use crate::operation::OperationState;
use crate::service::ServiceDefinition;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::{Supervisor, SupervisorLifecycleDeadlineKind, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, process, settings,
};

const SECOND_NS: u64 = 1_000_000_000;
const APP_ROOT: &str = "/sys/fs/cgroup/peinit/app";

struct NoBootAttemptCounter;

impl BootAttemptCounter for NoBootAttemptCounter {
    fn reset_boot_attempt_counter(&mut self) -> Result<(), crate::boundary::BoundaryError> {
        Ok(())
    }
}

/// A Notify service, as `svctl definition create` makes one: it never says
/// READY=1, so every start of it ends in a readiness timeout.
fn notify_app() -> ServiceDefinition {
    ServiceDefinition::simple_system_boot("app", "/sbin/app")
}

fn booted(app: ServiceDefinition) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot");
    supervisor
}

fn launch(supervisor: &mut Supervisor, at_ns: u64, pid: u32, pidfd: i32) -> String {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, pidfd)]);
    let mut clock = ScriptedClock::new([at_ns]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch")
        .expect("launch dispatch");
    let job = current_job(supervisor).id;
    supervisor.jobs().get(job).expect("job").cgroup_id.clone()
}

fn current_job(supervisor: &Supervisor) -> crate::control::query::CurrentJobView {
    supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("a main job")
}

fn current_operation(supervisor: &Supervisor) -> OperationId {
    supervisor
        .service_status("app")
        .expect("app")
        .current_operation
        .expect("an operation")
        .id
}

fn drive(
    supervisor: &mut Supervisor,
    controller: &mut TestProcessController,
    now_ns: u64,
) -> Option<crate::supervisor::SupervisorLifecycleDeadlineDispatch> {
    supervisor
        .process_due_lifecycle_deadlines(controller, &mut NoBootAttemptCounter, now_ns)
        .expect("a per-service deadline never fails the loop")
}

fn internal_errors(
    dispatch: &Option<crate::supervisor::SupervisorLifecycleDeadlineDispatch>,
) -> usize {
    dispatch
        .as_ref()
        .map_or(0, |dispatch| dispatch.internal_errors.len())
}

/// Stop a Starting service and let its main process exit.
fn stop(supervisor: &mut Supervisor, controller: &mut TestProcessController, at_ns: u64) {
    let job = current_job(supervisor).id;
    let mut clock = ScriptedClock::new([at_ns, at_ns + 1]);
    let accepted = supervisor
        .stop_service("app", None, &mut clock)
        .expect("stop");
    assert!(matches!(
        accepted.outcome,
        LifecycleCommandOutcome::OperationAccepted(_)
    ));
    supervisor
        .execute_next_pending_control_operation(controller, &mut clock)
        .expect("execute stop")
        .expect("stop dispatch");
    supervisor
        .complete_job(job, at_ns + 2, 0)
        .expect("main exits on SIGTERM");
}

/// The sequence from the bug, step for step: readiness timeout, backoff,
/// relaunch, stop, reset, definition change, reset, start.
#[test]
fn a_stop_during_a_restarts_start_leaves_nothing_behind_to_fail_the_next_start() {
    let mut supervisor = booted(notify_app());
    let mut controller = TestProcessController::default();
    launch(&mut supervisor, APP_LAUNCH_NS, 8000, 50);

    // The first start times out and the service backs off.
    let first = supervisor.next_readiness_timeout().expect("readiness");
    let timeout = drive(&mut supervisor, &mut controller, first.due_at_ns);
    assert_eq!(internal_errors(&timeout), 0);
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Backoff);
    assert_eq!(status.cause, Some(TransitionCause::ReadinessTimeout));

    // The restart relaunches inside the post-kill window, into the same
    // tree: nothing has leaked to move the generation on.
    let backoff = supervisor.next_restart_backoff_deadline().expect("backoff");
    drive(&mut supervisor, &mut controller, backoff.due_at_ns);
    assert_eq!(
        supervisor.service_status("app").expect("app").cause,
        Some(TransitionCause::RestartPolicy),
    );
    let relaunched = launch(&mut supervisor, backoff.due_at_ns + 10, 8001, 51);
    assert_eq!(relaunched, format!("{APP_ROOT}/main"));
    let restart = supervisor
        .next_readiness_timeout()
        .expect("restart readiness");
    assert_eq!(restart.operation_id, current_operation(&supervisor));

    // The first timeout's tree cleanup comes due while the restart is
    // living in that tree. It is not a leak.
    let cleanup_at = first.due_at_ns + 5 * SECOND_NS;
    assert!(cleanup_at < restart.due_at_ns);
    controller.cgroup_populated_checks.clear();
    let cleanup = drive(&mut supervisor, &mut controller, cleanup_at);
    assert!(
        cleanup
            .as_ref()
            .is_none_or(|dispatch| dispatch.cgroup_leaks.is_empty()),
        "{cleanup:?}",
    );
    assert!(
        !controller
            .cgroup_populated_checks
            .contains(&APP_ROOT.to_string()),
        "a tree in use is not probed for a leak: {:?}",
        controller.cgroup_populated_checks,
    );
    let status = supervisor.service_status("app").expect("app");
    assert!(status.warnings.is_empty(), "{:?}", status.warnings);
    assert!(status.lifecycle_warnings.is_empty());

    // Stop the restart while it waits for READY=1. The stop aborts it, and
    // its readiness deadline goes with it.
    stop(&mut supervisor, &mut controller, cleanup_at + 1_000);
    assert_eq!(
        supervisor
            .operation_status(restart.operation_id)
            .expect("restart")
            .state,
        OperationState::Aborted,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );
    assert!(supervisor.next_readiness_timeout().is_none());

    let mut clock = ScriptedClock::new([cleanup_at + 2_000]);
    supervisor
        .reset_service("app", None, &mut clock)
        .expect("reset");
    let mut changed = notify_app();
    changed.start_timeout_secs = 40;
    supervisor
        .reload_config_from_registry(&mut StaticRegistry::services(vec![changed]))
        .expect("definition change");
    let mut clock = ScriptedClock::new([cleanup_at + 3_000]);
    supervisor
        .reset_service("app", None, &mut clock)
        .expect("reset");

    // The next start runs in the tree it always had.
    let start_at = cleanup_at + 4_000;
    let mut clock = ScriptedClock::new([start_at]);
    supervisor
        .start_service("app", None, &mut clock)
        .expect("start");
    let started = launch(&mut supervisor, start_at + 1, 8002, 52);
    assert_eq!(started, format!("{APP_ROOT}/main"));
    let start = current_operation(&supervisor);
    let readiness = supervisor.next_readiness_timeout().expect("readiness");
    assert_eq!(readiness.operation_id, start);
    assert!(readiness.due_at_ns > restart.due_at_ns);

    // When the aborted restart's deadline would have come due: nothing.
    for turn in 0..3 {
        let dispatch = drive(&mut supervisor, &mut controller, restart.due_at_ns + turn);
        assert_eq!(internal_errors(&dispatch), 0, "turn {turn}: {dispatch:?}");
    }
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Starting);
    assert_eq!(
        supervisor.operation_status(start).expect("start").state,
        OperationState::Running,
    );
}

/// The deadline of a start that has already ended is dropped when it comes
/// due, whatever route left it behind, rather than acted on and contained.
#[test]
fn a_readiness_deadline_whose_start_has_ended_is_dropped_not_contained() {
    let mut supervisor = booted(notify_app());
    let mut controller = TestProcessController::default();
    launch(&mut supervisor, APP_LAUNCH_NS, 8000, 50);
    let first = supervisor.next_readiness_timeout().expect("readiness");
    stop(&mut supervisor, &mut controller, APP_LAUNCH_NS + 1_000);
    assert_eq!(
        supervisor
            .operation_status(first.operation_id)
            .expect("first start")
            .state,
        OperationState::Aborted,
    );

    let start_at = APP_LAUNCH_NS + 2_000;
    let mut clock = ScriptedClock::new([start_at]);
    supervisor
        .start_service("app", None, &mut clock)
        .expect("start");
    launch(&mut supervisor, start_at + 1, 8001, 51);
    let start = current_operation(&supervisor);

    // Planted behind the stop's back, as any route that forgets it would.
    supervisor
        .start
        .record_readiness_deadline(ReadinessDeadline {
            due_at_ns: first.due_at_ns,
            ..first.clone()
        });

    let dispatch = drive(&mut supervisor, &mut controller, first.due_at_ns);
    assert_eq!(internal_errors(&dispatch), 0, "{dispatch:?}");
    assert!(
        supervisor
            .start
            .readiness_deadline(first.operation_id)
            .is_none()
    );
    assert!(
        !controller.cgroup_kills.contains(&APP_ROOT.to_string()),
        "a stale deadline must not kill the tree the next start runs in",
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(
        supervisor.operation_status(start).expect("start").state,
        OperationState::Running,
    );
}

/// A deadline that raises is contained once. Containment fails the
/// operation before it clears deadlines, so the raising deadline was out of
/// its reach and was due again on the next turn, and the next.
#[test]
fn a_lifecycle_deadline_that_raises_is_contained_once_not_on_every_turn() {
    let mut supervisor = booted(notify_app());
    let mut controller = TestProcessController::default();
    launch(&mut supervisor, APP_LAUNCH_NS, 8000, 50);
    let readiness = supervisor.next_readiness_timeout().expect("readiness");
    controller.set_cgroup_kill_error(APP_ROOT, "Operation not permitted (os error 1)");

    let first = drive(&mut supervisor, &mut controller, readiness.due_at_ns);
    assert_eq!(internal_errors(&first), 1, "{first:?}");
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Failed);
    assert_eq!(status.cause, Some(TransitionCause::InternalError));

    assert!(
        !matches!(
            supervisor
                .next_lifecycle_deadline()
                .map(|deadline| deadline.kind),
            Some(SupervisorLifecycleDeadlineKind::ReadinessTimeout { .. })
        ),
        "the deadline that raised is gone",
    );
    for turn in 1..=3 {
        let again = drive(
            &mut supervisor,
            &mut controller,
            readiness.due_at_ns + turn * SECOND_NS,
        );
        assert_eq!(internal_errors(&again), 0, "turn {turn}: {again:?}");
    }
}
