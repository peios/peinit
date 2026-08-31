use crate::job::JobEvent;
use crate::operation::OperationType;
use crate::operation::store::OperationStore;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceTable, ServiceTableTransition};

use super::ended::EndedJob;
use super::model::{ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError};

pub(super) fn apply_stopping_simple_exit(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    service: &str,
    job_event: JobEvent,
    ended: EndedJob,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    let operation = operations
        .current_for_service(service)
        .ok_or_else(|| ServiceMainJobTerminalError::MissingStoppingOperation {
            service: service.to_string(),
        })?
        .clone();
    if !matches!(
        operation.operation_type,
        OperationType::Stop | OperationType::Restart
    ) {
        return Err(ServiceMainJobTerminalError::UnsupportedStoppingOperation {
            service: service.to_string(),
            operation_type: operation.operation_type,
        });
    }

    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let service_transition =
        apply_stopped_transition(&mut next_services, &operation.service, services)?;
    let operation_events = if operation.operation_type == OperationType::Stop {
        vec![
            next_operations
                .complete_operation(operation.id, ended.ended_at_ns, "inactive")
                .map_err(ServiceMainJobTerminalError::OperationStore)?,
        ]
    } else {
        Vec::new()
    };

    *services = next_services;
    *operations = next_operations;

    Ok(ServiceMainJobTerminalDispatch {
        job_event,
        operation_events,
        service_transitions: vec![service_transition],
        graph_events: Vec::new(),
        post_start_hook: None,
        late_exit: None,
    })
}

fn apply_stopped_transition(
    services: &mut ServiceTable,
    service: &str,
    current_services: &ServiceTable,
) -> Result<ServiceTableTransition, ServiceMainJobTerminalError> {
    let cause = current_services
        .runtime(service)
        .and_then(|runtime| runtime.cause)
        .unwrap_or(TransitionCause::ExplicitStop);
    services
        .transition_service(
            service,
            ServiceTransition {
                to: stopped_state(cause),
                cause: stopped_cause(cause),
            },
        )
        .map_err(ServiceMainJobTerminalError::ServiceTable)
}

fn stopped_state(cause: TransitionCause) -> ServiceState {
    match cause {
        TransitionCause::ConflictEviction | TransitionCause::BindsToPropagation => {
            ServiceState::Failed
        }
        _ => ServiceState::Inactive,
    }
}

fn stopped_cause(cause: TransitionCause) -> TransitionCause {
    match cause {
        TransitionCause::ConflictEviction
        | TransitionCause::BindsToPropagation
        | TransitionCause::ShutdownWave => cause,
        _ => TransitionCause::ExplicitStop,
    }
}
