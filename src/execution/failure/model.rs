use crate::execution::graph::{GraphExecutionError, GraphExecutionEvent};
use crate::ids::OperationId;
use crate::operation::store::{OperationEvent, OperationStoreError};
use crate::operation::{OperationState, OperationType};
use crate::service::runtime::TransitionCause;
use crate::service::{ServiceTableError, ServiceTableTransition};

use super::super::start_validation::RunningStartValidationError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartFailureRequest {
    pub service: String,
    pub operation_id: OperationId,
    pub failed_at_ns: u64,
    pub failure_cause: TransitionCause,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartFailureDispatch {
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartFailureError {
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
    ServiceTable(ServiceTableError),
    OperationStore(OperationStoreError),
    Graph(GraphExecutionError),
}

impl From<RunningStartValidationError> for StartFailureError {
    fn from(error: RunningStartValidationError) -> Self {
        match error {
            RunningStartValidationError::UnknownOperation { operation_id } => {
                Self::UnknownOperation { operation_id }
            }
            RunningStartValidationError::OperationServiceMismatch {
                operation_id,
                expected_service,
                actual_service,
            } => Self::OperationServiceMismatch {
                operation_id,
                expected_service,
                actual_service,
            },
            RunningStartValidationError::UnsupportedOperationType {
                operation_id,
                operation_type,
            } => Self::UnsupportedOperationType {
                operation_id,
                operation_type,
            },
            RunningStartValidationError::OperationNotRunning {
                operation_id,
                state,
            } => Self::OperationNotRunning {
                operation_id,
                state,
            },
        }
    }
}
