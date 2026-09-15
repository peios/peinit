//! Promotion of operations the conflict table queued (§8.3).
//!
//! A queued operation waits in the operation store, behind the one it was
//! queued behind, and is not handed to the control boundary until that one
//! is terminal. When it is, the queued operation is admitted afresh against
//! the state the service is in *now* -- which is the whole point of queueing:
//! a restart queued behind a stop finds an Inactive service and becomes a
//! start plan, exactly as `svctl restart` on an Inactive service would; a
//! restart queued behind a running start finds an Active service and goes to
//! the boundary for its stop leg. Dispatching it at request time ran it
//! against a service still mid-operation, and the InvalidTransition ended the
//! runtime loop (PEI-824).

use std::collections::BTreeSet;

use crate::control::lifecycle::{
    LifecycleCommandError, dispatch_existing_requested_start_plan, plan_on_demand_start,
};
use crate::execution::graph::GraphContextId;
use crate::execution::start::StartExecutionDispatch;
use crate::ids::OperationId;
use crate::operation::store::OperationEvent;
use crate::operation::{OperationRecord, OperationType};
use crate::service::runtime::ServiceState;

use super::super::control_boundary::{PendingControlOperation, queue_control_boundary_for};
use super::super::relationships::gate_start_context;
use super::super::state::{Supervisor, SupervisorError};
use super::super::work::SupervisorWork;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPromotedOperationDispatch {
    pub operation_id: OperationId,
    pub service: String,
    pub operation_type: OperationType,
    pub outcome: PromotedOperationOutcome,
    pub operation_events: Vec<OperationEvent>,
    pub context_id: Option<GraphContextId>,
    pub start_dispatches: Vec<StartExecutionDispatch>,
    pub pending_control_operation: Option<PendingControlOperation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotedOperationOutcome {
    /// Handed to the control boundary: the service is running and the
    /// operation acts on its process.
    ControlBoundary,
    /// Became an on-demand start plan: the service was not running.
    StartPlan,
    /// Left pending for another mechanism -- the backoff deadline, or the
    /// operation now ahead of it.
    Deferred,
    /// Completed on the spot: nothing to do.
    Completed,
    /// Failed on the spot: the service's state admits no such operation.
    Failed,
}

impl Supervisor {
    pub fn has_ready_queued_operations(&self) -> bool {
        !self.operations.ready_queued_operations().is_empty()
    }

    /// Admit every queued operation whose predecessor has finished, against
    /// the state its service is in now.
    pub fn promote_queued_operations(
        &mut self,
        now_ns: u64,
    ) -> Result<Vec<SupervisorPromotedOperationDispatch>, SupervisorError> {
        let ready = self.operations.ready_queued_operations();
        if ready.is_empty() {
            return Ok(Vec::new());
        }
        let max_parallel_starts = self.settings.phase2.max_parallel_starts;
        let mut work = SupervisorWork::from_supervisor(self);
        let mut dispatches = Vec::with_capacity(ready.len());
        for operation in ready {
            work.operations.clear_queued_behind(operation.id);
            dispatches.push(promote_one(
                &mut work,
                operation,
                now_ns,
                max_parallel_starts,
            )?);
        }
        work.commit(self);
        Ok(dispatches)
    }
}

fn promote_one(
    work: &mut SupervisorWork,
    operation: OperationRecord,
    now_ns: u64,
    max_parallel_starts: u32,
) -> Result<SupervisorPromotedOperationDispatch, SupervisorError> {
    let state = work
        .services
        .runtime(&operation.service)
        .map(|runtime| runtime.state);
    let mut dispatch = SupervisorPromotedOperationDispatch {
        operation_id: operation.id,
        service: operation.service.clone(),
        operation_type: operation.operation_type,
        outcome: PromotedOperationOutcome::Deferred,
        operation_events: Vec::new(),
        context_id: None,
        start_dispatches: Vec::new(),
        pending_control_operation: None,
    };

    match (operation.operation_type, state) {
        (
            OperationType::Stop | OperationType::Restart | OperationType::Reload,
            Some(
                ServiceState::Active
                | ServiceState::Reloading
                | ServiceState::Starting
                | ServiceState::Stopping,
            ),
        ) => {
            dispatch.pending_control_operation = queue_control_boundary_for(
                &mut work.pending_control_operations,
                &work.operations,
                &work.services,
                operation.id,
            );
            dispatch.outcome = PromotedOperationOutcome::ControlBoundary;
        }
        (
            OperationType::Start | OperationType::Restart,
            Some(
                ServiceState::Inactive
                | ServiceState::Completed
                | ServiceState::Failed
                | ServiceState::Skipped,
            ),
        ) => {
            // §8.1: a restart of a service with no process skips the stop
            // phase; the operation keeps its type. The existing record is the
            // requested start of the plan, as a deferred backoff start is.
            let plan =
                plan_on_demand_start(&work.services, &operation.service).map_err(|error| {
                    SupervisorError::Lifecycle(LifecycleCommandError::StartPlan(error))
                })?;
            let admission = dispatch_existing_requested_start_plan(
                &mut work.operations,
                &mut work.operation_ids,
                plan,
                operation.id,
                now_ns,
            )
            .map_err(|error| {
                SupervisorError::Lifecycle(LifecycleCommandError::StartDispatch(error))
            })?;
            let context_id = work
                .graph
                .create_on_demand_context(&admission, &work.services)
                .map_err(SupervisorError::GraphContext)?;
            let start_services: BTreeSet<String> = admission
                .plan
                .starts
                .iter()
                .map(|start| start.service.clone())
                .collect();
            dispatch.operation_events = admission.events.clone();
            dispatch.start_dispatches = gate_start_context(
                work,
                context_id,
                start_services,
                now_ns,
                max_parallel_starts,
            )?;
            dispatch.context_id = Some(context_id);
            dispatch.outcome = PromotedOperationOutcome::StartPlan;
        }
        (OperationType::Start | OperationType::Restart, Some(ServiceState::Backoff)) => {
            // The backoff deadline executes the pending operation (§10.3).
            dispatch.outcome = PromotedOperationOutcome::Deferred;
        }
        (
            OperationType::Stop,
            Some(
                ServiceState::Inactive
                | ServiceState::Completed
                | ServiceState::Failed
                | ServiceState::Skipped
                | ServiceState::Backoff,
            ),
        ) => {
            dispatch
                .operation_events
                .push(complete(work, operation.id, now_ns, "inactive")?);
            dispatch.outcome = PromotedOperationOutcome::Completed;
        }
        (
            OperationType::Start,
            Some(ServiceState::Active | ServiceState::Reloading | ServiceState::Starting),
        ) => {
            dispatch
                .operation_events
                .push(complete(work, operation.id, now_ns, "active")?);
            dispatch.outcome = PromotedOperationOutcome::Completed;
        }
        (OperationType::Start, Some(ServiceState::Stopping)) => {
            // Something else is stopping the service now; wait for it.
            work.operations.requeue_behind_head(operation.id);
            dispatch.outcome = PromotedOperationOutcome::Deferred;
        }
        (_, Some(state)) => {
            dispatch.operation_events.push(fail(
                work,
                operation.id,
                now_ns,
                format!(
                    "invalid_state: {:?} is not admitted while the service is {state:?}",
                    operation.operation_type
                ),
            )?);
            dispatch.outcome = PromotedOperationOutcome::Failed;
        }
        (_, None) => {
            dispatch.operation_events.push(fail(
                work,
                operation.id,
                now_ns,
                "unknown_service: the service is no longer defined",
            )?);
            dispatch.outcome = PromotedOperationOutcome::Failed;
        }
    }

    Ok(dispatch)
}

fn complete(
    work: &mut SupervisorWork,
    id: OperationId,
    now_ns: u64,
    result: &str,
) -> Result<OperationEvent, SupervisorError> {
    work.operations
        .complete_operation(id, now_ns, result)
        .map_err(|error| SupervisorError::Lifecycle(LifecycleCommandError::OperationStore(error)))
}

fn fail(
    work: &mut SupervisorWork,
    id: OperationId,
    now_ns: u64,
    reason: impl Into<String>,
) -> Result<OperationEvent, SupervisorError> {
    work.operations
        .fail_operation(id, now_ns, reason)
        .map_err(|error| SupervisorError::Lifecycle(LifecycleCommandError::OperationStore(error)))
}
