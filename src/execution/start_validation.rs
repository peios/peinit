use crate::ids::OperationId;
use crate::operation::store::OperationStore;
use crate::operation::{OperationSource, OperationState, OperationType};
use crate::service::runtime::TransitionCause;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunningStartValidationError {
    UnknownOperation {
        operation_id: OperationId,
    },
    OperationServiceMismatch {
        operation_id: OperationId,
        expected_service: String,
        actual_service: String,
    },
    UnsupportedOperationType {
        operation_id: OperationId,
        operation_type: OperationType,
    },
    OperationNotRunning {
        operation_id: OperationId,
        state: OperationState,
    },
}

pub(crate) fn validate_running_start_operation(
    operations: &OperationStore,
    service: &str,
    operation_id: OperationId,
) -> Result<(), RunningStartValidationError> {
    let record = operations
        .get(operation_id)
        .ok_or(RunningStartValidationError::UnknownOperation { operation_id })?;
    if record.service != service {
        return Err(RunningStartValidationError::OperationServiceMismatch {
            operation_id,
            expected_service: service.to_string(),
            actual_service: record.service.clone(),
        });
    }
    if !matches!(
        record.operation_type,
        OperationType::Start | OperationType::Restart
    ) {
        return Err(RunningStartValidationError::UnsupportedOperationType {
            operation_id,
            operation_type: record.operation_type,
        });
    }
    if record.state != OperationState::Running {
        return Err(RunningStartValidationError::OperationNotRunning {
            operation_id,
            state: record.state,
        });
    }
    Ok(())
}

pub(crate) fn start_transition_cause(
    operations: &OperationStore,
    operation_id: OperationId,
) -> Result<TransitionCause, RunningStartValidationError> {
    let record = operations
        .get(operation_id)
        .ok_or(RunningStartValidationError::UnknownOperation { operation_id })?;
    Ok(match record.source {
        OperationSource::DependencyPropagation => TransitionCause::DependencyStart,
        OperationSource::RestartPolicy => TransitionCause::RestartPolicy,
        OperationSource::BindsToRecovery => TransitionCause::BindsToRecovery,
        _ => TransitionCause::ExplicitStart,
    })
}
