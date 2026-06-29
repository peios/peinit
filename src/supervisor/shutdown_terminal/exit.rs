use crate::job::{JobEvent, JobEventDetail};
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceTableTransition, is_success_exit_code};
use crate::shutdown::{ShutdownError, ShutdownPlanError};

use super::super::work::SupervisorWork;

pub(super) fn apply_shutdown_job_exit(
    work: &mut SupervisorWork,
    job_event: &JobEvent,
) -> Result<Option<ServiceTableTransition>, ShutdownError> {
    let Some(service) = job_event.service.as_deref() else {
        return Ok(None);
    };
    remove_shutdown_deadlines_for_service(work, service)?;
    let Some(runtime) = work.services.runtime(service) else {
        return Ok(None);
    };
    let state = runtime.state;
    let cause = runtime.cause;
    if shutdown_exit_clears_fd_store(state) {
        work.fd_store.clear_service(service);
    }

    match state {
        ServiceState::Active => apply_shutdown_active_exit(work, service, job_event),
        ServiceState::Reloading => transition_shutdown_service(
            work,
            service,
            ServiceState::Failed,
            TransitionCause::ProcessCrash,
        ),
        ServiceState::Starting => transition_shutdown_service(
            work,
            service,
            ServiceState::Failed,
            TransitionCause::ShutdownWave,
        ),
        ServiceState::Stopping => {
            transition_shutdown_service(work, service, stopped_state(cause), stopped_cause(cause))
        }
        ServiceState::Failed | ServiceState::Abandoned | ServiceState::Inactive => Ok(None),
        _ => Ok(None),
    }
}

fn shutdown_exit_clears_fd_store(state: ServiceState) -> bool {
    matches!(
        state,
        ServiceState::Active
            | ServiceState::Reloading
            | ServiceState::Starting
            | ServiceState::Stopping
            | ServiceState::Abandoned
    )
}

fn apply_shutdown_active_exit(
    work: &mut SupervisorWork,
    service: &str,
    job_event: &JobEvent,
) -> Result<Option<ServiceTableTransition>, ShutdownError> {
    if shutdown_job_succeeded(work, service, job_event)? {
        return transition_shutdown_service(
            work,
            service,
            ServiceState::Inactive,
            TransitionCause::CleanExit,
        );
    }

    transition_shutdown_service(
        work,
        service,
        ServiceState::Failed,
        TransitionCause::ProcessCrash,
    )
}

fn shutdown_job_succeeded(
    work: &SupervisorWork,
    service: &str,
    job_event: &JobEvent,
) -> Result<bool, ShutdownError> {
    let definition = work.services.definition(service).ok_or_else(|| {
        ShutdownError::Plan(ShutdownPlanError::MissingDefinition {
            service: service.to_string(),
        })
    })?;
    let JobEventDetail::Ended {
        exit_code,
        exit_signal,
        failure_cause,
        ..
    } = &job_event.detail
    else {
        return Ok(false);
    };

    Ok(exit_signal.is_none()
        && failure_cause.is_none()
        && exit_code.is_some_and(|code| is_success_exit_code(definition, code)))
}

fn transition_shutdown_service(
    work: &mut SupervisorWork,
    service: &str,
    to: ServiceState,
    cause: TransitionCause,
) -> Result<Option<ServiceTableTransition>, ShutdownError> {
    work.services
        .transition_service(service, ServiceTransition { to, cause })
        .map(Some)
        .map_err(ShutdownError::ServiceTable)
}

fn stopped_state(cause: Option<TransitionCause>) -> ServiceState {
    match cause {
        Some(TransitionCause::ConflictEviction | TransitionCause::BindsToPropagation) => {
            ServiceState::Failed
        }
        _ => ServiceState::Inactive,
    }
}

fn stopped_cause(cause: Option<TransitionCause>) -> TransitionCause {
    match cause {
        Some(
            cause @ (TransitionCause::ConflictEviction
            | TransitionCause::BindsToPropagation
            | TransitionCause::ShutdownWave),
        ) => cause,
        _ => TransitionCause::ExplicitStop,
    }
}

fn remove_shutdown_deadlines_for_service(
    work: &mut SupervisorWork,
    service: &str,
) -> Result<(), ShutdownError> {
    let operation_ids = {
        let shutdown = work.shutdown_mut()?;
        let operation_ids = shutdown
            .stop_deadlines
            .iter()
            .filter(|deadline| deadline.service == service)
            .filter_map(|deadline| deadline.operation_id)
            .collect::<Vec<_>>();
        shutdown
            .stop_deadlines
            .retain(|deadline| deadline.service != service);
        shutdown
            .post_kill_deadlines
            .retain(|deadline| deadline.service != service);
        operation_ids
    };
    for operation_id in operation_ids {
        work.control.remove_stop_timeout(operation_id);
    }
    Ok(())
}
