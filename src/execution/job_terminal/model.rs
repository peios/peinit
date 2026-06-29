use crate::execution::failure::StartFailureError;
use crate::execution::graph::GraphExecutionEvent;
use crate::execution::start::StartExecutionError;
use crate::ids::JobId;
use crate::job::{JobEvent, JobType};
use crate::operation::OperationType;
use crate::operation::store::{OperationEvent, OperationStoreError};
use crate::service::runtime::ServiceState;
use crate::service::{ServiceTableError, ServiceTableTransition};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceMainJobTerminalDispatch {
    pub job_event: JobEvent,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub post_start_hook: Option<JobEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceMainJobTerminalError {
    NotServiceMainJob {
        job_id: JobId,
        job_type: JobType,
    },
    MissingService {
        job_id: JobId,
    },
    MissingOperation {
        job_id: JobId,
        service: String,
    },
    NotTerminalEvent {
        job_id: JobId,
    },
    UnsupportedServiceState {
        service: String,
        state: ServiceState,
    },
    MissingStoppingOperation {
        service: String,
    },
    UnsupportedStoppingOperation {
        service: String,
        operation_type: OperationType,
    },
    ServiceTable(ServiceTableError),
    OperationStore(OperationStoreError),
    Start(StartExecutionError),
    StartFailure(StartFailureError),
}
