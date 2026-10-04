use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::operation::OperationState;

use super::super::control_boundary::{PendingControlOperation, queue_control_boundary};
use super::super::dispatch::SupervisorLifecycleDispatch;
use super::super::fd_store_lifecycle::synchronous_clear_fd_store_service;
use super::super::held_starts::settle_held_restarts;
use super::super::relationships::gate_start_context;
use super::super::state::{Supervisor, SupervisorError};
use super::super::work::SupervisorWork;

pub(in crate::supervisor::lifecycle) fn finish_lifecycle_outcome(
    supervisor: &mut Supervisor,
    mut work: SupervisorWork,
    outcome: LifecycleCommandOutcome,
    max_parallel_starts: u32,
    observed_at_ns: u64,
) -> Result<SupervisorLifecycleDispatch, SupervisorError> {
    match &outcome {
        LifecycleCommandOutcome::OnDemandStart(dispatch) => {
            let context_id = work
                .graph
                .create_on_demand_context(dispatch, &work.services)
                .map_err(SupervisorError::GraphContext)?;
            let start_services = dispatch
                .plan
                .starts
                .iter()
                .map(|start| start.service.clone())
                .collect();
            let start_dispatches = gate_start_context(
                &mut work,
                context_id,
                start_services,
                observed_at_ns,
                max_parallel_starts,
            )?;
            work.commit(supervisor);
            Ok(SupervisorLifecycleDispatch {
                outcome,
                context_id: Some(context_id),
                start_dispatches,
                pending_control_operation: None,
                lifecycle_warnings: Vec::new(),
            })
        }
        LifecycleCommandOutcome::OperationAccepted(operation) => {
            drop_superseded_readiness_deadlines(&mut work, &operation.events);
            let pending_control_operation = queue_control_boundary(
                &mut work.pending_control_operations,
                &work.operations,
                &work.services,
                operation,
            );
            work.commit(supervisor);
            Ok(empty_lifecycle_dispatch(outcome, pending_control_operation))
        }
        LifecycleCommandOutcome::SynchronousClear(_) => {
            if let Some(service) = synchronous_clear_fd_store_service(&outcome) {
                work.fd_store.clear_service(service);
            }
            // A `stop` of a service in Backoff is synchronous and passes
            // through no operation of the graph's, so the dependents held
            // for its restart are settled here: it is not coming back.
            settle_held_restarts(&mut work, observed_at_ns, max_parallel_starts)?;
            work.commit(supervisor);
            Ok(empty_lifecycle_dispatch(outcome, None))
        }
        LifecycleCommandOutcome::Already(_) | LifecycleCommandOutcome::Noop(_) => {
            Ok(empty_lifecycle_dispatch(outcome, None))
        }
    }
}

/// A start this command superseded is no longer waiting for READY=1.
///
/// A `stop` of a Starting service aborts the start it lands on (§8.3), but
/// the readiness deadline the start armed is keyed by that operation and was
/// left in place. It came due later against an operation that was no longer
/// Running, raised, and — the operation being unreachable through the active
/// records — was never cleared: it failed whatever start of the service ran
/// next and then fired on every timer turn (PEI-1267). The main process is
/// the stop's to end; nothing is left for the deadline to time out.
fn drop_superseded_readiness_deadlines(
    work: &mut SupervisorWork,
    events: &[crate::operation::store::OperationEvent],
) {
    for event in events {
        if matches!(
            event.state,
            OperationState::Aborted | OperationState::Cancelled
        ) {
            work.start.remove_readiness_deadline(event.operation_id);
        }
    }
}

fn empty_lifecycle_dispatch(
    outcome: LifecycleCommandOutcome,
    pending_control_operation: Option<PendingControlOperation>,
) -> SupervisorLifecycleDispatch {
    SupervisorLifecycleDispatch {
        outcome,
        context_id: None,
        start_dispatches: Vec::new(),
        pending_control_operation,
        lifecycle_warnings: Vec::new(),
    }
}
