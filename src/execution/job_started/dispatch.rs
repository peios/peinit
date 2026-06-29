use crate::execution::start::{StartReadyContext, StartReadyRequest, complete_start_readiness};
use crate::job::{JobEvent, JobEventDetail, JobType};
use crate::service::runtime::ServiceState;
use crate::service::{Readiness, ServiceType};

use super::model::{ServiceMainJobStartedDispatch, ServiceMainJobStartedError};

pub fn apply_service_main_job_started(
    context: &mut StartReadyContext<'_>,
    job_event: JobEvent,
) -> Result<ServiceMainJobStartedDispatch, ServiceMainJobStartedError> {
    let started_at_ns = validate_started_service_main_event(&job_event)?;
    let service = service(&job_event)?;
    let definition = context
        .services
        .definition(&service)
        .ok_or_else(|| unknown_service(&service))?
        .clone();
    let state = context
        .services
        .runtime(&service)
        .ok_or_else(|| unknown_service(&service))?
        .state;

    if state != ServiceState::Starting {
        return Err(ServiceMainJobStartedError::UnsupportedServiceState { service, state });
    }

    match (definition.service_type, definition.readiness) {
        (ServiceType::Simple, Readiness::Alive) => {
            apply_alive_readiness(context, job_event, started_at_ns)
        }
        _ => Ok(noop_dispatch(job_event)),
    }
}

fn validate_started_service_main_event(
    event: &JobEvent,
) -> Result<u64, ServiceMainJobStartedError> {
    if event.job_type != JobType::ServiceMain {
        return Err(ServiceMainJobStartedError::NotServiceMainJob {
            job_id: event.job_id,
            job_type: event.job_type,
        });
    }
    if event.service.is_none() {
        return Err(ServiceMainJobStartedError::MissingService {
            job_id: event.job_id,
        });
    }
    let JobEventDetail::Started { started_at_ns, .. } = event.detail else {
        return Err(ServiceMainJobStartedError::NotStartedEvent {
            job_id: event.job_id,
        });
    };
    Ok(started_at_ns)
}

fn apply_alive_readiness(
    context: &mut StartReadyContext<'_>,
    job_event: JobEvent,
    started_at_ns: u64,
) -> Result<ServiceMainJobStartedDispatch, ServiceMainJobStartedError> {
    let service = service(&job_event)?;
    let operation_id =
        job_event
            .operation_id
            .ok_or_else(|| ServiceMainJobStartedError::MissingOperation {
                job_id: job_event.job_id,
                service: service.clone(),
            })?;
    let dispatch = complete_start_readiness(
        context,
        StartReadyRequest {
            service,
            operation_id,
            job_id: job_event.job_id,
            job_created_at_ns: job_event.created_at_ns,
            activation_generation: job_event.activation_generation,
            cgroup_generation: job_event.cgroup_generation,
            ready_at_ns: started_at_ns,
            result: "alive readiness: process started".to_string(),
        },
    )
    .map_err(ServiceMainJobStartedError::Start)?;

    Ok(ServiceMainJobStartedDispatch {
        job_event,
        operation_events: dispatch.operation_events,
        service_transitions: dispatch.service_transitions,
        graph_events: dispatch.graph_events,
        post_start_hook: dispatch.post_start_hook,
    })
}

fn noop_dispatch(job_event: JobEvent) -> ServiceMainJobStartedDispatch {
    ServiceMainJobStartedDispatch {
        job_event,
        operation_events: Vec::new(),
        service_transitions: Vec::new(),
        graph_events: Vec::new(),
        post_start_hook: None,
    }
}

fn service(job_event: &JobEvent) -> Result<String, ServiceMainJobStartedError> {
    job_event
        .service
        .clone()
        .ok_or(ServiceMainJobStartedError::MissingService {
            job_id: job_event.job_id,
        })
}

fn unknown_service(service: &str) -> ServiceMainJobStartedError {
    ServiceMainJobStartedError::ServiceTable(crate::service::ServiceTableError::UnknownService {
        service: service.to_string(),
    })
}
