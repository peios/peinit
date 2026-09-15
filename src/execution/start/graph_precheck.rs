use crate::execution::graph::GraphExecutionStore;
use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, ServiceTransition};
mod outcome;

use super::checks::{PreStartCheckDecision, evaluate_cacheable_pre_start_checks, tty_unavailable};
use super::deadline::start_operation_deadline_ns;
use super::initial::{InitialStartJobRequest, create_initial_start_job};
use super::job_id::job_id_for_request;
use super::model::{
    GraphPreStartCheckOutcome, GraphPreStartCheckTerminalDispatch, StartExecutionDispatch,
    StartExecutionError, StartExecutionRequest,
};

/// What a prechecked graph start turned into when its turn came.
///
/// Usually a job. But the pre-start check ran on a table where none of the
/// services released alongside this one had moved yet, so two boot-plan
/// services naming one `TTYPath` both passed it; the terminal is asked about
/// again here, at the moment of the transition, and the loser ends as the
/// check path would have ended it (PEI-808).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrecheckedReadyStartOutcome {
    Job(Box<StartExecutionDispatch>),
    Terminal(GraphPreStartCheckTerminalDispatch),
}
use super::skipped::clear_skipped_for_explicit_start;
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

    let cleared_skipped = clear_skipped_for_explicit_start(
        &mut transaction.services,
        &request.ready.service,
        request.ready.transition_cause,
    )?;
    let activation = transaction
        .services
        .prepare_activation_snapshot(&request.ready.service)
        .map_err(StartExecutionError::ServiceTable)?;

    let outcome = match evaluate_cacheable_pre_start_checks(
        &transaction.services,
        &request.ready.service,
        &activation.definition,
    ) {
        PreStartCheckDecision::Passed => {
            outcome::apply_passed(&mut transaction, request, activation, cleared_skipped)
        }
        PreStartCheckDecision::Skipped(reason) => {
            outcome::apply_skipped(&mut transaction, request, reason, cleared_skipped)
        }
        PreStartCheckDecision::AssertionFailed(check) => {
            outcome::apply_assertion_failed(&mut transaction, request, check, cleared_skipped)
        }
        PreStartCheckDecision::RequiresFilesystemHelper { checks } => {
            outcome::apply_filesystem_pending(
                &mut transaction,
                request,
                activation,
                checks,
                cleared_skipped,
            )
        }
    }?;

    transaction.commit_to_stores(services, operations, graph, start_store);

    Ok(outcome)
}

pub fn begin_prechecked_ready_start(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    jobs: &mut JobStore,
    job_ids: &mut JobIdAllocator,
    start_store: &mut StartExecutionStore,
    request: StartExecutionRequest,
) -> Result<PrecheckedReadyStartOutcome, StartExecutionError> {
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
    // Re-asked rather than carried over from the check: the check ran before
    // any service released in the same batch had transitioned, so a terminal
    // two boot-plan services both name looked free to both of them. Now the
    // earlier one is Starting and holds it.
    if let Some(reason) = tty_unavailable(
        &next_services,
        &request.ready.service,
        &prechecked.activation.definition,
    ) {
        let mut transaction = GraphPreStartCheckTransaction {
            services: next_services,
            operations: next_operations,
            graph: graph.clone(),
            start_store: next_start_store,
        };
        let outcome = outcome::apply_skipped(
            &mut transaction,
            request,
            reason,
            prechecked.cleared_skipped,
        )?;
        transaction.commit_to_stores(services, operations, graph, start_store);
        let GraphPreStartCheckOutcome::Terminal(terminal) = outcome else {
            unreachable!("a skipped pre-start check is a terminal outcome");
        };
        return Ok(PrecheckedReadyStartOutcome::Terminal(terminal));
    }
    if request.ready.released_from_hold {
        // Held on a fact with no clock until now (§7.5): the lifetime
        // starts at the release, not at a creation the hold outlasted.
        next_operations
            .restart_lifetime_clock(request.ready.operation_id, request.started_at_ns)
            .map_err(StartExecutionError::OperationStore)?;
    }
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

    Ok(PrecheckedReadyStartOutcome::Job(Box::new(
        StartExecutionDispatch {
            ready: request.ready,
            job_id: initial.job_id,
            operation_event,
            cleared_skipped: prechecked.cleared_skipped,
            service_transition,
            job_event: initial.job_event,
            job_kind: initial.job_kind,
        },
    )))
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
