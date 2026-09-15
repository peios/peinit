//! A start held on a readiness level lives until the level is decided
//! (PEI-830).
//!
//! §7.5: a held start does not time out — the service stays Inactive and
//! the start operation stays Pending, visible in `svctl status`. What ended
//! it was the operation lifetime of §8.2, which the maintenance sweep ran
//! against every Pending operation: once `StartTimeout` had passed since
//! the boot plan, both a hard and a soft level waiter were failed with
//! `operation_timeout`, the service left Inactive with no cause and no
//! operation. On a machine where the boot graph is the only context that
//! is also why the `Wants` waiter was never released by its publisher going
//! away later: there was nothing left to release.

use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::operation::OperationState;
use crate::service::ServiceDefinition;
use crate::service::runtime::ServiceState;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, oneshot_service, process,
    settings,
};

const NANOS_PER_SEC: u64 = 1_000_000_000;
const READY_NS: u64 = APP_LAUNCH_NS + 1_000;
/// Past the default `StartTimeout` (30 s) measured from the boot plan.
pub(super) const PAST_LIFETIME_NS: u64 = BOOT_NS + 31 * NANOS_PER_SEC;
const CONTROL_NS: u64 = LIFECYCLE_COMMAND_NS + 1;
const STOPPED_NS: u64 = CONTROL_NS + 1_000;

pub(super) fn datagram(pid: u32, payload: &[u8]) -> NotifyDatagram {
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

/// What a held start looks like, and the only thing it looks like: Inactive
/// with no cause, its start operation Pending and known to the graph as
/// held. Returns the operation, so a later look can check it is the same.
pub(super) fn assert_held(supervisor: &Supervisor, service: &str) -> crate::ids::OperationId {
    let status = supervisor.service_status(service).expect("status");
    assert_eq!(status.state, ServiceState::Inactive, "{service} is held");
    assert_eq!(
        status.cause, None,
        "{service} has neither started nor failed"
    );
    let operation = status
        .current_operation
        .expect("a held start keeps its operation");
    assert_eq!(operation.state, OperationState::Pending);
    assert!(supervisor.graph().is_operation_held(operation.id));
    operation.id
}

/// Boot a publisher (Notify readiness) with a hard level waiter and a soft
/// one on a level it never publishes, bring the publisher to Active, and
/// let the operation lifetime pass.
fn boot_level_waiters_past_the_lifetime(clock: &mut ScriptedClock) -> Supervisor {
    let netd = ServiceDefinition::simple_system_boot("netd", "/sbin/netd");
    let mut hold = oneshot_service("hold");
    hold.requires.push("netd:routed".to_string());
    let mut soft = oneshot_service("soft");
    soft.wants.push("netd:routed".to_string());

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![netd, hold, soft]);
    supervisor
        .run_phase2_boot(&mut registry, clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, clock)
        .expect("launch netd")
        .expect("netd launch dispatch");
    let mut controller = TestProcessController::default();
    supervisor
        .apply_notify_datagram(datagram(4242, b"READY=1"), READY_NS, &mut controller)
        .expect("netd ready");
    assert_eq!(
        supervisor.service_status("netd").expect("netd").state,
        ServiceState::Active
    );
    let hold_operation = assert_held(&supervisor, "hold");
    assert_held(&supervisor, "soft");

    // The PEI-830 mechanism: the maintenance sweep failed both held starts
    // with `operation_timeout` once StartTimeout had passed since the boot
    // plan, leaving each service Inactive with no cause and no operation.
    let maintenance = supervisor
        .process_due_operation_maintenance(PAST_LIFETIME_NS)
        .expect("maintenance");
    assert!(
        maintenance.operation_timeouts.is_empty(),
        "a held start does not time out"
    );
    assert!(
        !supervisor.operation_timeout_expired(hold_operation, PAST_LIFETIME_NS),
        "nor is a client waiting on it told that it has"
    );
    supervisor
}

fn stop_netd(supervisor: &mut Supervisor) {
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    supervisor
        .stop_service("netd", None, &mut clock)
        .expect("stop netd");
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute netd stop")
        .expect("netd stop dispatch");
    let netd_job = supervisor
        .service_status("netd")
        .expect("netd")
        .current_job
        .expect("netd job")
        .id;
    supervisor
        .complete_job(netd_job, STOPPED_NS, 0)
        .expect("netd stopped");
    assert_eq!(
        supervisor.service_status("netd").expect("netd").state,
        ServiceState::Inactive
    );
}

/// §7.5: a start held on a level survives its publisher going away. The
/// condition is still unmet, so the start is still pending — and still
/// visible.
#[test]
fn a_start_held_on_a_level_keeps_its_operation_when_the_publisher_stops() {
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    let mut supervisor = boot_level_waiters_past_the_lifetime(&mut clock);
    let held_operation = assert_held(&supervisor, "hold");

    stop_netd(&mut supervisor);

    assert_eq!(assert_held(&supervisor, "hold"), held_operation);
    let later = supervisor
        .process_due_operation_maintenance(STOPPED_NS + 60 * NANOS_PER_SEC)
        .expect("maintenance");
    assert!(later.operation_timeouts.is_empty());
    assert_eq!(assert_held(&supervisor, "hold"), held_operation);
}

/// §7.5: a `Wants` level waiter from the boot graph proceeds once its
/// publisher leaves a satisfying state — with no unrelated start to prime
/// anything, and however long after boot the publisher goes.
#[test]
fn a_boot_wants_level_waiter_is_released_when_its_publisher_stops() {
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    let mut supervisor = boot_level_waiters_past_the_lifetime(&mut clock);
    let held_operation = assert_held(&supervisor, "soft");

    stop_netd(&mut supervisor);

    let soft = supervisor.service_status("soft").expect("soft");
    assert_eq!(soft.state, ServiceState::Starting);
    assert_eq!(
        soft.current_operation
            .map(|operation| (operation.id, operation.state)),
        Some((held_operation, OperationState::Running))
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
}

/// A queued operation is not a held one: the lifetime still runs against a
/// start that is Pending behind another operation rather than in the
/// graph's hands.
#[test]
fn a_pending_operation_the_graph_does_not_hold_still_times_out() {
    let netd = ServiceDefinition::simple_system_boot("netd", "/sbin/netd");
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![netd]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS, LIFECYCLE_COMMAND_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch netd")
        .expect("netd launch dispatch");
    // A stop while netd is still Starting is admitted and queued for the
    // control boundary: Pending, and nothing in the graph holds it.
    let stop = supervisor
        .stop_service("netd", None, &mut clock)
        .expect("stop netd");
    let crate::control::lifecycle::LifecycleCommandOutcome::OperationAccepted(accepted) =
        &stop.outcome
    else {
        panic!("expected an accepted stop");
    };
    let stop_operation = accepted.returned_operation_id;
    assert!(!supervisor.graph().is_operation_held(stop_operation));

    let deadline = supervisor
        .next_operation_maintenance_deadline_ns()
        .expect("the queued stop has a lifetime");
    assert!(supervisor.operation_timeout_expired(stop_operation, deadline));
}
