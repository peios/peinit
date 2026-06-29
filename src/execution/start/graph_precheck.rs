use crate::execution::graph::GraphExecutionStore;
use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, ServiceTransition};
mod outcome;

use super::checks::{PreStartCheckDecision, evaluate_cacheable_pre_start_checks};
use super::deadline::start_operation_deadline_ns;
use super::initial::{InitialStartJobRequest, create_initial_start_job};
use super::job_id::job_id_for_request;
use super::model::{
    GraphPreStartCheckOutcome, StartExecutionDispatch, StartExecutionError, StartExecutionRequest,
};
use super::store::StartExecutionStore;

pub fn begin_graph_pre_start_check(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    start_store: &mut StartExecutionStore,
    request: StartExecutionRequest,
) -> Result<GraphPreStartCheckOutcome, StartExecutionError> {
    let mut transaction =
        GraphPreStartCheckTransaction::from_stores(services, operations, graph, start_store);

    let activation = transaction
        .services
        .prepare_activation_snapshot(&request.ready.service)
        .map_err(StartExecutionError::ServiceTable)?;

    let outcome = match evaluate_cacheable_pre_start_checks(
        &transaction.services,
        &activation.definition.conditions,
        &activation.definition.asserts,
    ) {
        PreStartCheckDecision::Passed => {
            outcome::apply_passed(&mut transaction, request, activation)
        }
        PreStartCheckDecision::ConditionSkipped(check) => {
            outcome::apply_condition_skipped(&mut transaction, request, check)
        }
        PreStartCheckDecision::AssertionFailed(check) => {
            outcome::apply_assertion_failed(&mut transaction, request, check)
        }
        PreStartCheckDecision::RequiresFilesystemHelper { checks } => {
            outcome::apply_filesystem_pending(&mut transaction, request, activation, checks)
        }
    }?;

    transaction.commit_to_stores(services, operations, graph, start_store);

    Ok(outcome)
}

pub fn begin_prechecked_ready_start(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    jobs: &mut JobStore,
    job_ids: &mut JobIdAllocator,
    start_store: &mut StartExecutionStore,
    request: StartExecutionRequest,
) -> Result<StartExecutionDispatch, StartExecutionError> {
    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_jobs = jobs.clone();
    let mut next_job_ids = job_ids.clone();
    let mut next_start_store = start_store.clone();

    let prechecked = next_start_store
        .remove_prechecked_graph_start(request.ready.operation_id)
        .ok_or(StartExecutionError::MissingPrecheckedGraphStart {
            operation_id: request.ready.operation_id,
        })?;
    let operation_event = next_operations
        .start_operation(request.ready.operation_id, request.started_at_ns)
        .map_err(StartExecutionError::OperationStore)?;
    let operation_deadline_ns = start_operation_deadline_ns(
        next_operations
            .get(request.ready.operation_id)
            .ok_or(
                crate::operation::store::OperationStoreError::UnknownOperation {
                    id: request.ready.operation_id,
                },
            )
            .map_err(StartExecutionError::OperationStore)?,
        &prechecked.activation.definition,
        request.started_at_ns,
    );
    let main_job_id = job_id_for_request(&mut next_job_ids, &request)?;
    let service_transition = next_services
        .transition_service(
            &request.ready.service,
            ServiceTransition {
                to: ServiceState::Starting,
                cause: request.ready.transition_cause,
            },
        )
        .map_err(StartExecutionError::ServiceTable)?;
    let initial = create_initial_start_job(
        &mut next_jobs,
        &mut next_job_ids,
        &mut next_start_store,
        InitialStartJobRequest {
            service: &request.ready.service,
            operation_id: request.ready.operation_id,
            resolved_identity: prechecked.resolved_identity,
            token_summary: prechecked.token_summary,
            started_at_ns: request.started_at_ns,
            operation_deadline_ns,
            activation: &prechecked.activation,
            main_job_id,
        },
    )?;

    *services = next_services;
    *operations = next_operations;
    *jobs = next_jobs;
    *job_ids = next_job_ids;
    *start_store = next_start_store;

    Ok(StartExecutionDispatch {
        ready: request.ready,
        job_id: initial.job_id,
        operation_event,
        service_transition,
        job_event: initial.job_event,
        job_kind: initial.job_kind,
    })
}

struct GraphPreStartCheckTransaction {
    services: ServiceTable,
    operations: OperationStore,
    graph: GraphExecutionStore,
    start_store: StartExecutionStore,
}

impl GraphPreStartCheckTransaction {
    fn from_stores(
        services: &ServiceTable,
        operations: &OperationStore,
        graph: &GraphExecutionStore,
        start_store: &StartExecutionStore,
    ) -> Self {
        Self {
            services: services.clone(),
            operations: operations.clone(),
            graph: graph.clone(),
            start_store: start_store.clone(),
        }
    }

    fn commit_to_stores(
        self,
        services: &mut ServiceTable,
        operations: &mut OperationStore,
        graph: &mut GraphExecutionStore,
        start_store: &mut StartExecutionStore,
    ) {
        *services = self.services;
        *operations = self.operations;
        *graph = self.graph;
        *start_store = self.start_store;
    }
}
