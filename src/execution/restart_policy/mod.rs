#[cfg(test)]
mod tests;

use std::collections::VecDeque;

use crate::control::lifecycle::{
    OnDemandStartDispatch, dispatch_existing_requested_start_plan, plan_restart_policy_start,
};
use crate::control::restart_policy::{
    RestartPolicyAdmissionError, admit_due_restart_policy_start, ensure_due_restart_backoff,
};
use crate::execution::graph::{
    GraphContextBuildError, GraphContextId, GraphExecutionError, GraphExecutionStore,
};
use crate::execution::start::{
    StartExecutionDispatch, StartExecutionError, StartExecutionOutcome, StartExecutionRequest,
    StartExecutionStore, begin_ready_start,
};
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::job::JobStore;
use crate::operation::store::OperationStore;
use crate::operation::{OperationState, OperationType};
use crate::security::TokenSummary;
use crate::service::ServiceTable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPolicyRelaunchRequest {
    pub service: String,
    pub observed_at_ns: u64,
    pub max_parallel_starts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPolicyRelaunchDispatch {
    pub admission: OnDemandStartDispatch,
    pub context_id: GraphContextId,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

pub struct RestartPolicyRelaunchContext<'a> {
    pub services: &'a mut ServiceTable,
    pub operations: &'a mut OperationStore,
    pub graph: &'a mut GraphExecutionStore,
    pub jobs: &'a mut JobStore,
    pub start_store: &'a mut StartExecutionStore,
    pub operation_ids: &'a mut OperationIdAllocator,
    pub job_ids: &'a mut JobIdAllocator,
}

pub fn begin_due_restart_policy_relaunch(
    context: &mut RestartPolicyRelaunchContext<'_>,
    request: RestartPolicyRelaunchRequest,
) -> Result<RestartPolicyRelaunchDispatch, RestartPolicyRelaunchError> {
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_start_store = context.start_store.clone();
    let mut next_operation_ids = context.operation_ids.clone();
    let mut next_job_ids = context.job_ids.clone();

    let admission = admit_restart_backoff_start(
        &next_services,
        &mut next_operations,
        &mut next_operation_ids,
        &request.service,
        request.observed_at_ns,
    )?;
    let context_id = next_graph
        .create_on_demand_context(&admission, &next_services)
        .map_err(RestartPolicyRelaunchError::GraphContext)?;
    let mut start_dispatches = Vec::new();
    let mut context_ids = VecDeque::from([context_id]);
    while let Some(context_id) = context_ids.pop_front() {
        let ready = next_graph
            .release_ready(context_id, request.max_parallel_starts, &|target, level| {
                crate::execution::graph::probe_level(&next_services, target, level)
            })
            .map_err(RestartPolicyRelaunchError::GraphExecution)?;

        for ready in ready {
            let definition = next_services.definition(&ready.service).ok_or_else(|| {
                RestartPolicyRelaunchError::MissingStartCredentials {
                    service: ready.service.clone(),
                }
            })?;
            let resolved_identity = definition.identity.clone();
            let token_summary = TokenSummary::requested_identity(resolved_identity.clone());
            let outcome = begin_ready_start(
                &mut next_services,
                &mut next_operations,
                &mut next_graph,
                &mut next_jobs,
                &mut next_job_ids,
                &mut next_start_store,
                StartExecutionRequest {
                    ready,
                    resolved_identity,
                    token_summary,
                    started_at_ns: request.observed_at_ns,
                },
            )
            .map_err(RestartPolicyRelaunchError::Start)?;
            match outcome {
                StartExecutionOutcome::Job(dispatch) => start_dispatches.push(*dispatch),
                StartExecutionOutcome::Terminal(dispatch) => {
                    context_ids.extend(dispatch.graph_events.iter().map(|event| event.context_id));
                }
                StartExecutionOutcome::CheckPending(_) => {}
            }
        }
    }

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.jobs = next_jobs;
    *context.start_store = next_start_store;
    *context.operation_ids = next_operation_ids;
    *context.job_ids = next_job_ids;

    Ok(RestartPolicyRelaunchDispatch {
        admission,
        context_id,
        start_dispatches,
    })
}

fn admit_restart_backoff_start(
    services: &ServiceTable,
    operations: &mut OperationStore,
    operation_ids: &mut OperationIdAllocator,
    service: &str,
    observed_at_ns: u64,
) -> Result<OnDemandStartDispatch, RestartPolicyRelaunchError> {
    if let Some(operation_id) = pending_deferred_start(operations, service) {
        ensure_due_restart_backoff(services, service, observed_at_ns)
            .map_err(RestartPolicyRelaunchError::Admission)?;
        let plan = plan_restart_policy_start(services, service).map_err(|error| {
            RestartPolicyRelaunchError::Admission(RestartPolicyAdmissionError::StartPlan(error))
        })?;
        return dispatch_existing_requested_start_plan(
            operations,
            operation_ids,
            plan,
            operation_id,
            observed_at_ns,
        )
        .map_err(RestartPolicyRelaunchError::StartDispatch);
    }

    admit_due_restart_policy_start(services, operations, operation_ids, service, observed_at_ns)
        .map_err(RestartPolicyRelaunchError::Admission)
}

fn pending_deferred_start(
    operations: &OperationStore,
    service: &str,
) -> Option<crate::ids::OperationId> {
    let operation = operations.current_for_service(service)?;
    (operation.operation_type == OperationType::Start && operation.state == OperationState::Pending)
        .then_some(operation.id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartPolicyRelaunchError {
    Admission(RestartPolicyAdmissionError),
    GraphContext(GraphContextBuildError),
    GraphExecution(GraphExecutionError),
    MissingStartCredentials { service: String },
    StartDispatch(crate::control::lifecycle::OnDemandStartDispatchError),
    Start(StartExecutionError),
}
