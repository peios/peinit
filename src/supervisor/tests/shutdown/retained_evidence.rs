//! Peinit TRM §12.2 step 4, "Timing an already-stopping service": the
//! evidence a participant already in Stopping is timed by, and what happens
//! when it cannot substantiate a deadline.

use crate::service::runtime::{ServiceStoppingTimeoutEvidence, TransitionCause};
use crate::shutdown::ShutdownKind;
use crate::supervisor::Supervisor;

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{
    DRAINING_STOP_DEADLINE_NS, LATER_WAVE_STOP_DEADLINE_NS, job_for, later_wave_stopping_fixture,
    shutdown_fixture,
};

/// The fixture's `draining` with its in-flight stop operation gone, so the
/// service-level evidence is the only evidence there is.
fn draining_on_service_evidence(evidence: Option<ServiceStoppingTimeoutEvidence>) -> Supervisor {
    let mut supervisor = shutdown_fixture();
    let operation_id = supervisor
        .operations
        .current_for_service("draining")
        .expect("draining stop operation")
        .id;
    supervisor.control.remove_stop_timeout(operation_id);
    supervisor
        .operations
        .complete_operation(operation_id, SHUTDOWN_NS - 1, "inactive")
        .expect("complete retained operation");
    if let Some(evidence) = evidence {
        supervisor
            .services
            .record_stopping_timeout("draining", evidence)
            .expect("service-level stop evidence");
    }
    supervisor
}

/// Begin the shutdown and return what the first wave made of `draining`:
/// why its evidence was refused, if it was, and the deadline it was given.
fn draining_outcome(supervisor: &mut Supervisor) -> (Option<&'static str>, u64) {
    let mut controller = TestProcessController::default();
    let dispatch = supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("one participant's evidence must not fail the shutdown");
    let draining = dispatch
        .first_wave
        .iter()
        .find(|stop| stop.service == "draining")
        .expect("draining participant");
    assert!(draining.already_stopping, "draining joined the wave as already stopping");
    assert!(draining.signal.is_none(), "and was not sent another SIGTERM");
    (
        draining.unsubstantiated_deadline,
        draining.deadline.as_ref().expect("a deadline").due_at_ns,
    )
}

// The rule is that the evidence must be present, belong to a service
// actually in Stopping, carry a cause matching the service's current cause,
// name one of the four stop causes, and not describe a deadline earlier than
// its own start — and that evidence failing any of them gives the participant
// no graceful period (a deadline already due) rather than a guessed one.
//
// One condition in turn per case, against a control case that meets them
// all. The fourth condition is not exercised: a service enters Stopping only
// under a stop cause (service/runtime/rules.rs), and evidence whose cause
// differs from the service's is refused by the third condition first, so no
// evidence reaching the fourth check can fail it without a seam that sets a
// runtime's state and cause directly.
#[test]
fn retained_evidence_failing_any_condition_gets_no_graceful_period() {
    let started_at_ns = SHUTDOWN_NS - 2_000_000_000;

    // Control: evidence that meets every condition governs the deadline.
    let mut valid = draining_on_service_evidence(Some(ServiceStoppingTimeoutEvidence {
        started_at_ns,
        due_at_ns: DRAINING_STOP_DEADLINE_NS,
        cause: TransitionCause::ExplicitStop,
    }));
    assert_eq!(
        draining_outcome(&mut valid),
        (None, DRAINING_STOP_DEADLINE_NS),
        "evidence meeting every condition is used as it stands",
    );

    // Present.
    let mut missing = draining_on_service_evidence(None);
    assert_eq!(
        draining_outcome(&mut missing),
        (Some("no retained stopping timeout"), SHUTDOWN_NS),
    );

    // Its cause matches the service's current cause.
    let mut mismatched = draining_on_service_evidence(Some(ServiceStoppingTimeoutEvidence {
        started_at_ns,
        due_at_ns: DRAINING_STOP_DEADLINE_NS,
        cause: TransitionCause::ShutdownWave,
    }));
    assert_eq!(
        draining_outcome(&mut mismatched),
        (
            Some("retained timeout's cause does not match the service's"),
            SHUTDOWN_NS
        ),
    );

    // Not a deadline earlier than its own start.
    let mut backwards = draining_on_service_evidence(Some(ServiceStoppingTimeoutEvidence {
        started_at_ns,
        due_at_ns: started_at_ns - 1,
        cause: TransitionCause::ExplicitStop,
    }));
    assert_eq!(
        draining_outcome(&mut backwards),
        (Some("retained timeout is due before it started"), SHUTDOWN_NS),
    );

    // Belongs to a service actually in Stopping. Only a later wave can meet
    // a participant planned as already stopping that has since left
    // Stopping: `back` finishes its stop while wave 0 is still running. A
    // Stopping service only ever leaves for a done state, and a participant
    // already done is skipped when its wave begins (PEI-1086) — so `back`'s
    // stale evidence is never consulted, and it gets neither a deadline nor
    // a kill for a process that has already gone.
    let mut supervisor = later_wave_stopping_fixture();
    let mut controller = TestProcessController::default();
    let back_job = job_for(&supervisor, "back");
    let front_job = job_for(&supervisor, "front");
    let begun = supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    assert_eq!(
        begun.runtime.plan.stop_waves[1]
            .services
            .iter()
            .map(|participant| (participant.service.as_str(), participant.already_stopping))
            .collect::<Vec<_>>(),
        vec![("back", true), ("peer", false)],
        "back is planned into wave 1 as already stopping",
    );
    supervisor
        .complete_shutdown_job(back_job, SHUTDOWN_NS + 1, 0, &mut controller)
        .expect("back finishes its stop");
    assert_ne!(
        supervisor.service_status("back").expect("back").state,
        crate::service::runtime::ServiceState::Stopping,
        "back has left Stopping before its wave",
    );
    // As in the cases above, the service-level evidence is the one on
    // trial: the explicit stop's own retained timing is taken away, so no
    // in-flight operation stands in front of it.
    let back_operation = supervisor
        .operations
        .current_for_service("back")
        .expect("back's stop operation")
        .id;
    supervisor.control.remove_stop_timeout(back_operation);
    supervisor
        .operations
        .complete_operation(back_operation, SHUTDOWN_NS + 1, "inactive")
        .expect("complete back's stop");
    supervisor
        .services
        .record_stopping_timeout(
            "back",
            ServiceStoppingTimeoutEvidence {
                started_at_ns,
                due_at_ns: LATER_WAVE_STOP_DEADLINE_NS,
                cause: TransitionCause::ExplicitStop,
            },
        )
        .expect("stale evidence for a service no longer stopping");
    let front_done = supervisor
        .complete_shutdown_job(front_job, SHUTDOWN_NS + 2, 0, &mut controller)
        .expect("front finishes, wave 1 begins");
    assert!(
        !front_done
            .next_wave
            .iter()
            .any(|stop| stop.service == "back"),
        "back has already stopped, so wave 1 records nothing for it",
    );
    assert!(
        !supervisor
            .shutdown()
            .expect("shutdown")
            .stop_deadlines
            .iter()
            .any(|deadline| deadline.service == "back"),
        "and holds no deadline for it",
    );
    let peer = front_done
        .next_wave
        .iter()
        .find(|stop| stop.service == "peer")
        .expect("peer in wave 1");
    assert_eq!(peer.unsubstantiated_deadline, None);
    assert!(
        peer.deadline.as_ref().expect("peer deadline").due_at_ns > SHUTDOWN_NS + 2,
        "while the rest of the wave keeps its full StopTimeout",
    );
}
