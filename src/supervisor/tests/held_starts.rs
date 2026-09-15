//! The dependents of a service in Backoff wait for its restart (PEI-821).
//!
//! §6.1: a service in Backoff is *going* to start again, and its dependents
//! wait rather than failing. A dependent held this way looks exactly like a
//! start held on a readiness level — Inactive, start operation Pending,
//! exempt from the operation lifetime (see `held_level_waiters`) — and is
//! released when the target reaches a dependent-satisfying state, or failed
//! with `DependencyFailure` when the target gives up: restart budget
//! exhausted, stopped, or withdrawn.

use crate::operation::OperationState;
use crate::service::ServiceDefinition;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::{
    Supervisor, SupervisorHeldRestartAbandonReason, SupervisorHeldRestartOutcome,
    SupervisorSettings,
};

use super::held_level_waiters::{PAST_LIFETIME_NS, assert_held, datagram};
use super::{
    APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, oneshot_service, process,
    settings,
};

const CRASH_NS: u64 = APP_LAUNCH_NS + 2_000;
const RELAUNCH_NS: u64 = PAST_LIFETIME_NS + 1_000;
const RELAUNCH_READY_NS: u64 = RELAUNCH_NS + 1_000;
const DEPENDENT_LAUNCH_NS: u64 = RELAUNCH_READY_NS + 1_000;
const STOPPED_NS: u64 = LIFECYCLE_COMMAND_NS + 1_000;

/// Boot `target` (Notify readiness) with `dependent`, launch the target and
/// crash it before it reports ready: the target is in Backoff and the
/// dependent is whatever the graph decided for it.
fn boot_and_crash_target(
    target: ServiceDefinition,
    dependent: ServiceDefinition,
    clock: &mut ScriptedClock,
) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![target, dependent]);
    supervisor
        .run_phase2_boot(&mut registry, clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, clock)
        .expect("launch target")
        .expect("target launch dispatch");
    let target_job = supervisor
        .service_status("target")
        .expect("target")
        .current_job
        .expect("target job")
        .id;
    supervisor
        .complete_job(target_job, CRASH_NS, 1)
        .expect("target crashed before ready");
    assert_eq!(
        supervisor.service_status("target").expect("target").state,
        ServiceState::Backoff
    );
    supervisor
}

fn hard_dependent() -> ServiceDefinition {
    let mut dependent = oneshot_service("dependent");
    dependent.requires.push("target".to_string());
    dependent
}

/// §6.1: a service in Backoff is going to start again, and its dependents
/// wait rather than failing. The dependent looks exactly like a start held
/// on a readiness level, survives the operation lifetime, and starts when
/// the relaunch reaches Active — under the operation it had all along.
#[test]
fn a_hard_dependent_of_a_target_in_backoff_waits_and_starts_when_it_comes_back() {
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS, RELAUNCH_NS, DEPENDENT_LAUNCH_NS]);
    let target = ServiceDefinition::simple_system_boot("target", "/sbin/target");
    let mut supervisor = boot_and_crash_target(target, hard_dependent(), &mut clock);
    let held_operation = assert_held(&supervisor, "dependent");

    // The lifetime that times out a queued operation does not run against
    // a held one (§7.5).
    let maintenance = supervisor
        .process_due_operation_maintenance(PAST_LIFETIME_NS)
        .expect("maintenance");
    assert!(maintenance.operation_timeouts.is_empty());
    assert_eq!(assert_held(&supervisor, "dependent"), held_operation);
    assert!(!supervisor.operation_timeout_expired(held_operation, PAST_LIFETIME_NS));

    let deadline = supervisor
        .next_restart_backoff_deadline()
        .expect("restart deadline");
    let relaunches = supervisor
        .process_due_restart_backoffs(deadline.due_at_ns)
        .expect("due restart scan");
    assert_eq!(relaunches.len(), 1);
    assert_eq!(
        supervisor.service_status("target").expect("target").state,
        ServiceState::Starting
    );
    assert_eq!(
        assert_held(&supervisor, "dependent"),
        held_operation,
        "still held while the relaunch is in flight"
    );

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4243, 10), process(4300, 11)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch relaunch")
        .expect("relaunch dispatch");
    let mut controller = TestProcessController::default();
    let ready = supervisor
        .apply_notify_datagram(
            datagram(4243, b"READY=1"),
            RELAUNCH_READY_NS,
            &mut controller,
        )
        .expect("target ready")
        .into_service()
        .expect("service notify outcome");
    assert_eq!(
        supervisor.service_status("target").expect("target").state,
        ServiceState::Active
    );
    assert_eq!(
        ready
            .start_dispatches
            .iter()
            .map(|dispatch| (dispatch.ready.service.as_str(), dispatch.ready.operation_id))
            .collect::<Vec<_>>(),
        vec![("dependent", held_operation)],
        "the target coming back releases the dependent under its own operation"
    );
    assert!(!supervisor.graph().is_awaiting_restart("target"));

    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch dependent")
        .expect("dependent dispatch");
    let dependent = supervisor.service_status("dependent").expect("dependent");
    assert_eq!(dependent.state, ServiceState::Starting);
    assert_eq!(
        dependent
            .current_operation
            .map(|operation| (operation.id, operation.state)),
        Some((held_operation, OperationState::Running))
    );
    assert!(
        supervisor.take_held_restart_settlements().is_empty(),
        "a hold settled by the relaunch's own operation is reported by that path"
    );
}

/// Failing the dependents waits for the give-up: the relaunch crashing on an
/// exhausted budget takes the target to Failed, and only then do the
/// dependents held for it fail with `DependencyFailure`.
#[test]
fn a_hard_dependent_fails_only_when_its_target_exhausts_its_restart_budget() {
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS, RELAUNCH_NS]);
    let mut target = ServiceDefinition::simple_system_boot("target", "/sbin/target");
    target.restart_max_retries = 1;
    let mut supervisor = boot_and_crash_target(target, hard_dependent(), &mut clock);
    let held_operation = assert_held(&supervisor, "dependent");

    let deadline = supervisor
        .next_restart_backoff_deadline()
        .expect("restart deadline");
    supervisor
        .process_due_restart_backoffs(deadline.due_at_ns)
        .expect("due restart scan");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4243, 10)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch relaunch")
        .expect("relaunch dispatch");
    assert_eq!(assert_held(&supervisor, "dependent"), held_operation);

    let relaunch_job = supervisor
        .service_status("target")
        .expect("target")
        .current_job
        .expect("relaunch job")
        .id;
    let terminal = supervisor
        .complete_job(relaunch_job, RELAUNCH_NS + 1_000, 1)
        .expect("relaunch crashed");

    let target = supervisor.service_status("target").expect("target");
    assert_eq!(target.state, ServiceState::Failed);
    assert_eq!(target.cause, Some(TransitionCause::RestartBudgetExhausted));
    let dependent = supervisor.service_status("dependent").expect("dependent");
    assert_eq!(dependent.state, ServiceState::Failed);
    assert_eq!(dependent.cause, Some(TransitionCause::DependencyFailure));
    let operation = supervisor
        .operation_status(held_operation)
        .expect("dependent operation");
    assert_eq!(operation.state, OperationState::Failed);
    assert!(
        terminal
            .terminal
            .service_transitions
            .iter()
            .any(|transition| transition.event.service == "dependent"
                && transition.event.to == ServiceState::Failed),
        "the dependent's failure rides on the relaunch's terminal dispatch"
    );
}

/// An operator stopping a service in Backoff is the other give-up. The stop
/// is synchronous and passes through no operation of the graph's, so the
/// hold is settled from the state, and the evidence is reported separately.
#[test]
fn stopping_a_target_in_backoff_fails_the_dependents_held_for_it() {
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    let target = ServiceDefinition::simple_system_boot("target", "/sbin/target");
    let mut supervisor = boot_and_crash_target(target, hard_dependent(), &mut clock);
    let held_operation = assert_held(&supervisor, "dependent");

    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    supervisor
        .stop_service("target", None, &mut clock)
        .expect("stop target");
    assert_eq!(
        supervisor.service_status("target").expect("target").state,
        ServiceState::Inactive
    );

    let dependent = supervisor.service_status("dependent").expect("dependent");
    assert_eq!(dependent.state, ServiceState::Failed);
    assert_eq!(dependent.cause, Some(TransitionCause::DependencyFailure));
    let operation = supervisor
        .operation_status(held_operation)
        .expect("dependent operation");
    assert_eq!(operation.state, OperationState::Failed);
    assert!(
        operation
            .error
            .as_deref()
            .is_some_and(|error| error.contains("target was stopped")),
        "the error says why: {:?}",
        operation.error
    );

    let settlements = supervisor.take_held_restart_settlements();
    assert_eq!(settlements.len(), 1);
    assert_eq!(settlements[0].target, "target");
    assert_eq!(
        settlements[0].outcome,
        SupervisorHeldRestartOutcome::Abandoned(SupervisorHeldRestartAbandonReason::Stopped)
    );
    assert_eq!(settlements[0].service_transitions.len(), 1);
    assert_eq!(settlements[0].operation_events.len(), 1);
    assert!(supervisor.take_held_restart_settlements().is_empty());
}

/// `Wants` keeps its semantics with a hold: the soft dependent waits while
/// the target is coming back, and proceeds once it is not.
#[test]
fn a_wants_dependent_of_a_target_in_backoff_waits_and_proceeds_when_the_target_is_stopped() {
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    let target = ServiceDefinition::simple_system_boot("target", "/sbin/target");
    let mut dependent = oneshot_service("dependent");
    dependent.wants.push("target".to_string());
    let mut supervisor = boot_and_crash_target(target, dependent, &mut clock);
    let held_operation = assert_held(&supervisor, "dependent");
    assert!(supervisor.pending_launch_jobs().is_empty());

    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    supervisor
        .stop_service("target", None, &mut clock)
        .expect("stop target");

    let dependent = supervisor.service_status("dependent").expect("dependent");
    assert_eq!(dependent.state, ServiceState::Starting);
    assert_eq!(
        dependent
            .current_operation
            .map(|operation| (operation.id, operation.state)),
        Some((held_operation, OperationState::Running))
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
    let settlements = supervisor.take_held_restart_settlements();
    assert_eq!(settlements.len(), 1);
    assert!(settlements[0].service_transitions.is_empty());
    assert_eq!(
        settlements[0]
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["dependent"]
    );
}

/// A definition withdrawn on reload discards a Backoff entry, on a path
/// with no transition funnel: the work pump's reconciliation finds the
/// decided hold and settles it.
#[test]
fn a_target_withdrawn_in_backoff_is_settled_by_the_reconciliation_pass() {
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    let target = ServiceDefinition::simple_system_boot("target", "/sbin/target");
    let mut supervisor = boot_and_crash_target(target, hard_dependent(), &mut clock);
    let held_operation = assert_held(&supervisor, "dependent");
    assert!(!supervisor.has_settleable_held_restarts());
    // The boot did its part: a target in Backoff and the dependent held for
    // it do not keep the boot window (§3.7) open, so the reload is admitted.
    assert!(!supervisor.boot_plan_in_progress());

    // The reload refuses a graph with a dangling hard edge, so the dependent
    // is re-declared without it. Its start was planned against the old
    // definition and is still held on the target, which is now gone.
    let mut without_target = StaticRegistry::services(vec![oneshot_service("dependent")]);
    supervisor
        .reload_config_from_registry(&mut without_target)
        .expect("reload without target");
    assert!(supervisor.service_status("target").is_err());
    assert_eq!(assert_held(&supervisor, "dependent"), held_operation);
    assert!(supervisor.has_settleable_held_restarts());

    supervisor
        .settle_held_restarts_now(STOPPED_NS)
        .expect("settle");
    assert!(!supervisor.has_settleable_held_restarts());
    assert!(!supervisor.graph().is_awaiting_restart("target"));
    assert_eq!(
        supervisor
            .operation_status(held_operation)
            .expect("dependent operation")
            .state,
        OperationState::Failed
    );
    let settlements = supervisor.take_held_restart_settlements();
    assert_eq!(settlements.len(), 1);
    assert_eq!(
        settlements[0].outcome,
        SupervisorHeldRestartOutcome::Abandoned(SupervisorHeldRestartAbandonReason::Withdrawn)
    );
}
