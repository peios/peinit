use crate::control::lifecycle::LifecycleCommandOutcome;

use super::super::control_boundary::{PendingControlOperation, queue_control_boundary};
use super::super::dispatch::SupervisorLifecycleDispatch;
use super::super::fd_store_lifecycle::synchronous_clear_fd_store_service;
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
            work.commit(supervisor);
            Ok(empty_lifecycle_dispatch(outcome, None))
        }
        LifecycleCommandOutcome::Already(_) | LifecycleCommandOutcome::Noop(_) => {
            Ok(empty_lifecycle_dispatch(outcome, None))
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
