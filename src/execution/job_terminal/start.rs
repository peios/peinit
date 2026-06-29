use crate::execution::failure::{StartFailureDispatch, StartFailureRequest, apply_start_failure};
use crate::job::JobEvent;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::TransitionCause;

use super::super::graph::GraphExecutionStore;
use super::super::start::{
    StartReadyContext, StartReadyDispatch, StartReadyRequest, complete_start_readiness,
};
use super::ended::EndedJob;
use super::model::{ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError};

pub(super) fn apply_oneshot_start_success(
    context: &mut StartReadyContext<'_>,
    service: &str,
    job_event: JobEvent,
    ended: EndedJob,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    let operation_id = operation_id(&job_event, service)?;

    let dispatch = complete_start_readiness(
        context,
        StartReadyRequest {
            service: service.to_string(),
            operation_id,
            job_id: job_event.job_id,
            job_created_at_ns: job_event.created_at_ns,
            activation_generation: job_event.activation_generation,
            cgroup_generation: job_event.cgroup_generation,
            ready_at_ns: ended.ended_at_ns,
            result: ended.result(),
        },
    )
    .map_err(ServiceMainJobTerminalError::Start)?;

    Ok(dispatch_from_readiness(job_event, dispatch))
}

pub(super) fn apply_start_process_failure(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    graph: &mut GraphExecutionStore,
    service: &str,
    job_event: JobEvent,
    ended: EndedJob,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    let operation_id = operation_id(&job_event, service)?;
    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_graph = graph.clone();

    let dispatch = apply_start_failure(
        &mut next_services,
        &mut next_operations,
        &mut next_graph,
        StartFailureRequest {
            service: service.to_string(),
            operation_id,
            failed_at_ns: ended.ended_at_ns,
            failure_cause: TransitionCause::ProcessCrash,
            reason: ended.failure_reason(),
        },
    )
    .map_err(ServiceMainJobTerminalError::StartFailure)?;

    *services = next_services;
    *operations = next_operations;
    *graph = next_graph;

    Ok(dispatch_from_failure(job_event, dispatch))
}

fn operation_id(
    job_event: &JobEvent,
    service: &str,
) -> Result<crate::ids::OperationId, ServiceMainJobTerminalError> {
    job_event
        .operation_id
        .ok_or_else(|| ServiceMainJobTerminalError::MissingOperation {
            job_id: job_event.job_id,
            service: service.to_string(),
        })
}

fn dispatch_from_readiness(
    job_event: JobEvent,
    dispatch: StartReadyDispatch,
) -> ServiceMainJobTerminalDispatch {
    ServiceMainJobTerminalDispatch {
        job_event,
        operation_events: dispatch.operation_events,
        service_transitions: dispatch.service_transitions,
        graph_events: dispatch.graph_events,
        post_start_hook: dispatch.post_start_hook,
    }
}

fn dispatch_from_failure(
    job_event: JobEvent,
    dispatch: StartFailureDispatch,
) -> ServiceMainJobTerminalDispatch {
    ServiceMainJobTerminalDispatch {
        job_event,
        operation_events: dispatch.operation_events,
        service_transitions: dispatch.service_transitions,
        graph_events: dispatch.graph_events,
        post_start_hook: None,
    }
}
