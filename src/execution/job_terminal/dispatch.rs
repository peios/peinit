use crate::job::JobEvent;
use crate::service::ServiceType;
use crate::service::runtime::ServiceState;

use super::super::start::StartReadyContext;
use super::active::{apply_active_simple_exit, apply_reloading_simple_exit};
use super::ended::validate_terminal_service_main_event;
use super::model::{ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError};
use super::start::{apply_oneshot_start_success, apply_start_process_failure};
use super::stopping::apply_stopping_simple_exit;

pub fn apply_service_main_job_terminal(
    context: &mut StartReadyContext<'_>,
    job_event: JobEvent,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    let terminal = validate_terminal_service_main_event(&job_event)?;
    let service = terminal.service;
    let ended = terminal.ended;
    let definition = context
        .services
        .definition(&service)
        .ok_or_else(|| {
            ServiceMainJobTerminalError::ServiceTable(
                crate::service::ServiceTableError::UnknownService {
                    service: service.clone(),
                },
            )
        })?
        .clone();
    let state = context
        .services
        .runtime(&service)
        .ok_or_else(|| {
            ServiceMainJobTerminalError::ServiceTable(
                crate::service::ServiceTableError::UnknownService {
                    service: service.clone(),
                },
            )
        })?
        .state;

    let succeeded = ended.is_success_for(&definition);
    match (state, definition.service_type, succeeded) {
        (ServiceState::Starting, ServiceType::Oneshot, true) => {
            apply_oneshot_start_success(context, &service, job_event, ended)
        }
        (ServiceState::Starting, _, _) => apply_start_process_failure(
            context.services,
            context.operations,
            context.graph,
            &service,
            job_event,
            ended,
        ),
        (ServiceState::Active, ServiceType::Simple, _) => {
            apply_active_simple_exit(context.services, &service, job_event, &definition, ended)
        }
        (ServiceState::Reloading, ServiceType::Simple, _) => {
            apply_reloading_simple_exit(context.services, &service, job_event, &definition, ended)
        }
        (ServiceState::Stopping, ServiceType::Simple, _) => apply_stopping_simple_exit(
            context.services,
            context.operations,
            &service,
            job_event,
            ended,
        ),
        _ => Err(ServiceMainJobTerminalError::UnsupportedServiceState { service, state }),
    }
}
