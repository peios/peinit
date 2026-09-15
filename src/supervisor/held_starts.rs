//! The lifetime of a held start.
//!
//! A start is *held* while the fact its dependency edge waits on is
//! undecided: a readiness level not yet published (§7.5), or a target in
//! Backoff that is going to start again (§6.1). Both look the same from the
//! outside — the service is Inactive and its start operation is Pending,
//! visible in `svctl status` — and both are re-evaluated by the same release
//! pass over the graph context. A hold ends only when that fact is decided:
//! the level arrives or its publisher leaves (§7.5), the target comes back
//! or gives up (this module). Nothing else ends it: not the operation
//! lifetime that times out a queued operation (§8.2, exempted in the
//! maintenance deadlines), and not the target merely leaving a
//! dependent-satisfying state (PEI-830).
//!
//! A target in Backoff decides its dependents by *state*, because the
//! restart it owes them runs under an operation the holding context does
//! not own. Every terminal operation settles the holds on its service on
//! the way through the graph (`apply_operation_terminal`), which covers the
//! relaunch reaching Active, Completed or Skipped, and failing for good.
//! What is left are the routes on which a service leaves Backoff with no
//! operation of its own through the graph: an operator's `stop` (synchronous
//! from Backoff), a definition withdrawn on reload (the entry is discarded),
//! and a due restart peinit could not execute. This pass settles those from
//! the service table, and is run from the relationship funnel and from each
//! of those routes.

use crate::execution::failure::fail_dependents_after_graph_events;
use crate::execution::graph::GraphTerminalOutcome;
use crate::execution::start::StartExecutionError;
use crate::service::ServiceTable;
use crate::service::runtime::ServiceState;

use super::dispatch::{
    SupervisorHeldRestartAbandonReason, SupervisorHeldRestartOutcome,
    SupervisorHeldRestartSettlementDispatch,
};
use super::relationships::apply_relationship_reactions_after_transitions;
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

/// Settle every hold whose target's state has decided it.
///
/// A target still in Backoff, or Starting under its relaunch, keeps its
/// dependents held. One that reached a dependent-satisfying state releases
/// them; one that is Failed, Inactive, Abandoned or gone fails the hard
/// ones with `DependencyFailure` and frees the soft ones. What was settled
/// is recorded on the work snapshot for the runtime to report, since the
/// routes that need this pass carry no dispatch of their own for it.
pub(in crate::supervisor) fn settle_held_restarts(
    work: &mut SupervisorWork,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<(), SupervisorError> {
    for target in work.graph.awaiting_restart_services() {
        let Some(outcome) = decided_outcome(&work.services, &target) else {
            continue;
        };
        let graph_outcome = match outcome {
            SupervisorHeldRestartOutcome::Released => GraphTerminalOutcome::Satisfied,
            SupervisorHeldRestartOutcome::Abandoned(_) => GraphTerminalOutcome::Failed,
        };
        let graph_events = work
            .graph
            .settle_awaiting_restart(&target, graph_outcome)
            .map_err(SupervisorError::Graph)?;

        let mut dispatch = SupervisorHeldRestartSettlementDispatch {
            target: target.clone(),
            outcome,
            operation_events: Vec::new(),
            service_transitions: Vec::new(),
            graph_events: graph_events.clone(),
            start_dispatches: Vec::new(),
        };
        if let SupervisorHeldRestartOutcome::Abandoned(reason) = outcome {
            let failures = fail_dependents_after_graph_events(
                &mut work.services,
                &mut work.operations,
                &graph_events,
                &target,
                observed_at_ns,
                &abandon_reason(&target, reason),
            )
            .map_err(|error| SupervisorError::Start(StartExecutionError::StartFailure(error)))?;
            dispatch.operation_events = failures.operation_events;
            dispatch.service_transitions = failures.service_transitions;
        }
        // A dependent that just failed is a failure like any other: its
        // OnFailure handler starts, its own dependents fail. And a soft
        // waiter now sees a terminal member either way.
        dispatch
            .start_dispatches
            .extend(apply_relationship_reactions_after_transitions(
                work,
                &dispatch.service_transitions,
                observed_at_ns,
                max_parallel_starts,
            )?);
        dispatch
            .start_dispatches
            .extend(work.release_after_graph_events(
                &graph_events,
                max_parallel_starts,
                observed_at_ns,
            )?);
        work.held_restart_settlements.push(dispatch);
    }
    Ok(())
}

impl Supervisor {
    /// Whether some hold on a service in Backoff has been decided by that
    /// service's state and not yet settled — a target withdrawn on reload
    /// is the route with no hook of its own.
    pub fn has_settleable_held_restarts(&self) -> bool {
        self.graph
            .awaiting_restart_services()
            .iter()
            .any(|target| decided_outcome(&self.services, target).is_some())
    }

    /// Settle every decided hold, as a transaction of its own. The work pump
    /// runs this before and after each event turn, so no route that moves a
    /// service out of Backoff can leave its dependents held.
    pub fn settle_held_restarts_now(&mut self, now_ns: u64) -> Result<(), SupervisorError> {
        let mut work = SupervisorWork::from_supervisor(self);
        settle_held_restarts(&mut work, now_ns, self.settings.phase2.max_parallel_starts)?;
        work.commit(self);
        Ok(())
    }
}

fn decided_outcome(services: &ServiceTable, target: &str) -> Option<SupervisorHeldRestartOutcome> {
    let Some(runtime) = services.runtime(target) else {
        return Some(SupervisorHeldRestartOutcome::Abandoned(
            SupervisorHeldRestartAbandonReason::Withdrawn,
        ));
    };
    if runtime.state.satisfies_dependents() {
        return Some(SupervisorHeldRestartOutcome::Released);
    }
    match runtime.state {
        // Between attempts, or attempting: the restart is still owed.
        ServiceState::Backoff | ServiceState::Starting | ServiceState::Stopping => None,
        ServiceState::Inactive => Some(SupervisorHeldRestartOutcome::Abandoned(
            SupervisorHeldRestartAbandonReason::Stopped,
        )),
        ServiceState::Failed | ServiceState::Abandoned => Some(
            SupervisorHeldRestartOutcome::Abandoned(SupervisorHeldRestartAbandonReason::Failed),
        ),
        // Dependent-satisfying states were answered above.
        ServiceState::Active
        | ServiceState::Reloading
        | ServiceState::Completed
        | ServiceState::Skipped => Some(SupervisorHeldRestartOutcome::Released),
    }
}

fn abandon_reason(target: &str, reason: SupervisorHeldRestartAbandonReason) -> String {
    let what = match reason {
        SupervisorHeldRestartAbandonReason::Stopped => "was stopped",
        SupervisorHeldRestartAbandonReason::Withdrawn => "had its definition withdrawn",
        SupervisorHeldRestartAbandonReason::Failed => "failed",
    };
    format!(
        "DependencyFailure: dependency {target} {what} while its dependents waited for its restart"
    )
}
