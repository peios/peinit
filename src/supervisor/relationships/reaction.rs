use crate::execution::start::StartExecutionDispatch;
use crate::service::ServiceTableTransition;

use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

mod binds_to;
mod conflict;
mod on_failure;

use binds_to::{apply_binds_to_propagation, apply_binds_to_recovery_starts};
use conflict::release_unblocked_conflict_contexts;
use on_failure::{apply_on_failure_starts, clear_finished_on_failure_chains};

/// A service leaving a dependent-satisfying state stops making level
/// claims. That cannot open a `Requires = ["svc:level"]` gate (that takes
/// the level itself, over the notify socket), but it is exactly what opens
/// a `Wants` one: soft level waiters proceed once nobody could still
/// publish the level. This funnel sees every transition — crash, stop,
/// eviction — so the waiters are re-evaluated wherever the change came
/// from, not only on notify traffic.
fn release_level_waiters_after_transitions(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    let mut dispatches = Vec::new();
    for transition in transitions {
        let event = &transition.event;
        if !event.from.satisfies_dependents() || event.to.satisfies_dependents() {
            continue;
        }
        dispatches.extend(work.release_level_waiters_on(
            &event.service,
            max_parallel_starts,
            observed_at_ns,
        )?);
    }
    Ok(dispatches)
}

pub(in crate::supervisor) fn apply_relationship_reactions_after_transitions(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    clear_finished_on_failure_chains(work, transitions);
    if work.shutdown.is_some() {
        return Ok(Vec::new());
    }

    apply_binds_to_propagation(work, transitions, observed_at_ns)?;

    let mut start_dispatches = Vec::new();
    start_dispatches.extend(apply_on_failure_starts(
        work,
        transitions,
        observed_at_ns,
        max_parallel_starts,
    )?);
    start_dispatches.extend(apply_binds_to_recovery_starts(
        work,
        transitions,
        observed_at_ns,
        max_parallel_starts,
    )?);
    start_dispatches.extend(release_unblocked_conflict_contexts(
        work,
        observed_at_ns,
        max_parallel_starts,
    )?);
    start_dispatches.extend(release_level_waiters_after_transitions(
        work,
        transitions,
        observed_at_ns,
        max_parallel_starts,
    )?);
    Ok(start_dispatches)
}
