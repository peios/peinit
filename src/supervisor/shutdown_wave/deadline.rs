use crate::operation::{OperationState, OperationType};
use crate::service::ServiceTable;
use crate::service::runtime::TransitionCause;
use crate::shutdown::{ShutdownError, ShutdownPlanError, ShutdownStopDeadline};

use super::super::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(super) fn retained_stop_deadline(
    work: &SupervisorWork,
    service: &str,
    wave_index: usize,
) -> Result<ShutdownStopDeadline, ShutdownError> {
    if let Some(operation) = work
        .operations
        .current_for_service(service)
        .filter(|operation| {
            operation.state == OperationState::Running
                && matches!(
                    operation.operation_type,
                    OperationType::Stop | OperationType::Restart
                )
        })
    {
        let retained = work.control.stop_timeout(operation.id).ok_or_else(|| {
            ShutdownError::MissingStoppingTimeoutEvidence {
                service: service.to_string(),
            }
        })?;

        return Ok(ShutdownStopDeadline {
            service: service.to_string(),
            cgroup_id: service_root_cgroup_id(&work.services, service)?,
            started_at_ns: operation.started_at_ns.ok_or_else(|| {
                ShutdownError::MissingStoppingTimeoutEvidence {
                    service: service.to_string(),
                }
            })?,
            due_at_ns: retained.due_at_ns,
            wave: wave_index,
            operation_id: Some(operation.id),
        });
    }

    let runtime = work.services.runtime(service).ok_or_else(|| {
        ShutdownError::Plan(ShutdownPlanError::MissingRuntime {
            service: service.to_string(),
        })
    })?;
    let retained = runtime.stopping_timeout.as_ref().ok_or_else(|| {
        ShutdownError::MissingStoppingTimeoutEvidence {
            service: service.to_string(),
        }
    })?;
    if runtime.state != crate::service::runtime::ServiceState::Stopping
        || runtime.cause != Some(retained.cause)
        || !retained_stop_cause(retained.cause)
        || retained.due_at_ns < retained.started_at_ns
    {
        return Err(ShutdownError::MissingStoppingTimeoutEvidence {
            service: service.to_string(),
        });
    }

    Ok(ShutdownStopDeadline {
        service: service.to_string(),
        cgroup_id: service_root_cgroup_id(&work.services, service)?,
        started_at_ns: retained.started_at_ns,
        due_at_ns: retained.due_at_ns,
        wave: wave_index,
        operation_id: None,
    })
}

fn retained_stop_cause(cause: TransitionCause) -> bool {
    matches!(
        cause,
        TransitionCause::ExplicitStop
            | TransitionCause::ShutdownWave
            | TransitionCause::ConflictEviction
            | TransitionCause::BindsToPropagation
    )
}

pub(in crate::supervisor) fn service_root_cgroup_id(
    services: &ServiceTable,
    service: &str,
) -> Result<String, ShutdownError> {
    let runtime = services.runtime(service).ok_or_else(|| {
        ShutdownError::Plan(ShutdownPlanError::MissingRuntime {
            service: service.to_string(),
        })
    })?;
    Ok(crate::job::service_cgroup_root_path(
        service,
        runtime.cgroup_generation,
    ))
}

pub(super) fn stop_deadline_ns(
    services: &ServiceTable,
    service: &str,
    started_at_ns: u64,
) -> Result<u64, ShutdownError> {
    let definition = services.definition(service).ok_or_else(|| {
        ShutdownError::Plan(ShutdownPlanError::MissingDefinition {
            service: service.to_string(),
        })
    })?;
    Ok(started_at_ns.saturating_add(definition.stop_timeout_secs.saturating_mul(NANOS_PER_SEC)))
}
