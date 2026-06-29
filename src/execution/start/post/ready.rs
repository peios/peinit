use crate::execution::command::parse_executable_command;
use crate::execution::satisfaction::{
    StartSatisfactionDispatch, StartSatisfactionRequest, apply_start_satisfaction,
};
use crate::execution::start_validation::{
    start_transition_cause, validate_running_start_operation,
};
use crate::security::{TokenSummary, hook_execution_identity};
use crate::service::runtime::{ServiceState, ServiceTransition};
use crate::service::{ServiceDefinition, ServiceTable, ServiceTableError, ServiceType};

use super::super::deadline::{post_start_hook_deadline, start_operation_deadline_ns};
use super::super::model::{
    StartExecutionError, StartReadyContext, StartReadyDispatch, StartReadyRequest,
};
use super::super::store::PostStartHookSequence;
use super::job::post_start_hook_job;

pub fn complete_start_readiness(
    context: &mut StartReadyContext<'_>,
    request: StartReadyRequest,
) -> Result<StartReadyDispatch, StartExecutionError> {
    let definition = context
        .services
        .definition(&request.service)
        .ok_or_else(|| {
            StartExecutionError::ServiceTable(ServiceTableError::UnknownService {
                service: request.service.clone(),
            })
        })?
        .clone();
    let commands = parse_post_start_hooks(&definition)?;
    if commands.is_empty() {
        let dispatch = apply_start_satisfaction(
            context.services,
            context.operations,
            context.graph,
            StartSatisfactionRequest {
                service: request.service,
                operation_id: request.operation_id,
                satisfied_at_ns: request.ready_at_ns,
                result: request.result,
            },
        )
        .map_err(StartExecutionError::StartSatisfaction)?;
        return Ok(dispatch_from_satisfaction(dispatch));
    }

    let mut next_services = context.services.clone();
    let next_operations = context.operations.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_job_ids = context.job_ids.clone();
    let mut next_start_store = context.start_store.clone();

    validate_running_start_operation(&next_operations, &request.service, request.operation_id)
        .map_err(|error| StartExecutionError::StartSatisfaction(error.into()))?;
    let operation_deadline_ns = start_operation_deadline_ns(
        next_operations
            .get(request.operation_id)
            .ok_or(
                crate::operation::store::OperationStoreError::UnknownOperation {
                    id: request.operation_id,
                },
            )
            .map_err(StartExecutionError::OperationStore)?,
        &definition,
        request.job_created_at_ns,
    );
    let transition = next_services
        .transition_service(
            &request.service,
            ServiceTransition {
                to: satisfied_state(&next_services, &request.service)?,
                cause: start_transition_cause(&next_operations, request.operation_id)
                    .map_err(|error| StartExecutionError::StartSatisfaction(error.into()))?,
            },
        )
        .map_err(StartExecutionError::ServiceTable)?;
    let job_id = next_job_ids
        .allocate_batch(1, request.ready_at_ns)
        .map(|ids| ids[0])
        .map_err(StartExecutionError::JobIdAllocation)?;
    let Some(first_command) = commands.first().cloned() else {
        return Err(StartExecutionError::EmptyExecStartPostSequence {
            service: request.service.clone(),
        });
    };
    let hook_identity = hook_execution_identity(&definition);
    let sequence = PostStartHookSequence {
        service: request.service.clone(),
        operation_id: request.operation_id,
        definition,
        resolved_identity: hook_identity.clone(),
        token_summary: TokenSummary::requested_identity(hook_identity),
        activation_generation: request.activation_generation,
        cgroup_generation: request.cgroup_generation,
        commands,
        next_index: 1,
        deadline_ns: operation_deadline_ns,
        readiness_result: request.result,
        had_failure: false,
    };
    let job = post_start_hook_job(&sequence, job_id, first_command, 0, request.ready_at_ns)?;
    let job_event = next_jobs
        .create_job(job)
        .map_err(StartExecutionError::JobStore)?;
    next_start_store.record_post_start_sequence(sequence.clone());
    next_start_store.record_post_start_hook_deadline(post_start_hook_deadline(&sequence, job_id));

    *context.services = next_services;
    *context.operations = next_operations;
    *context.jobs = next_jobs;
    *context.job_ids = next_job_ids;
    *context.start_store = next_start_store;

    Ok(StartReadyDispatch {
        operation_events: Vec::new(),
        service_transitions: vec![transition],
        graph_events: Vec::new(),
        post_start_hook: Some(job_event),
    })
}

fn parse_post_start_hooks(
    definition: &ServiceDefinition,
) -> Result<Vec<Vec<String>>, StartExecutionError> {
    definition
        .exec_start_post
        .iter()
        .enumerate()
        .map(|(command_index, command)| {
            parse_executable_command(command).map_err(|source| {
                StartExecutionError::InvalidExecStartPostCommand {
                    service: definition.name.clone(),
                    command_index,
                    command: command.clone(),
                    source,
                }
            })
        })
        .collect()
}

fn dispatch_from_satisfaction(dispatch: StartSatisfactionDispatch) -> StartReadyDispatch {
    StartReadyDispatch {
        operation_events: vec![dispatch.operation_event],
        service_transitions: dispatch.service_transitions,
        graph_events: dispatch.graph_events,
        post_start_hook: None,
    }
}

fn satisfied_state(
    services: &ServiceTable,
    service: &str,
) -> Result<ServiceState, StartExecutionError> {
    let definition = services.definition(service).ok_or_else(|| {
        StartExecutionError::ServiceTable(ServiceTableError::UnknownService {
            service: service.to_string(),
        })
    })?;
    Ok(match definition.service_type {
        ServiceType::Simple => ServiceState::Active,
        ServiceType::Oneshot => ServiceState::Completed,
    })
}
