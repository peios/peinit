use crate::execution::graph::{GraphExecutionError, GraphExecutionEvent};
use crate::ids::OperationId;
use crate::operation::store::{OperationEvent, OperationStoreError};
use crate::operation::{OperationState, OperationType};
use crate::service::{ServiceTableError, ServiceTableTransition};

use super::super::start_validation::RunningStartValidationError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSatisfactionRequest {
    pub service: String,
    pub operation_id: OperationId,
    pub satisfied_at_ns: u64,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSatisfactionDispatch {
    pub operation_event: OperationEvent,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartSatisfactionError {
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

impl From<RunningStartValidationError> for StartSatisfactionError {
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
