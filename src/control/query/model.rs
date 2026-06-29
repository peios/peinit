use crate::ids::{JobId, OperationId};
use crate::job::JobType;
use crate::operation::{OperationSource, OperationState, OperationType};
use crate::service::runtime::{ServiceHealthStatus, ServiceState, TransitionCause};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatusView {
    pub service: String,
    pub state: ServiceState,
    pub cause: Option<TransitionCause>,
    pub generation: u64,
    pub status_text: Option<String>,
    pub health: Option<ServiceHealthStatus>,
    pub definition_removed: bool,
    pub current_job: Option<CurrentJobView>,
    pub current_operation: Option<CurrentOperationView>,
    pub warnings: Vec<ServiceStatusWarning>,
    pub lifecycle_warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceListItem {
    pub service: String,
    pub state: ServiceState,
    pub cause: Option<TransitionCause>,
    pub health: Option<ServiceHealthStatus>,
    pub definition_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentJobView {
    pub id: JobId,
    pub job_type: JobType,
    pub pid: Option<u32>,
    pub started_at_ns: Option<u64>,
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentOperationView {
    pub id: OperationId,
    pub operation_type: OperationType,
    pub source: OperationSource,
    pub state: OperationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatusWarning {
    pub path: String,
    pub warning_type: ServiceStatusWarningType,
    pub detected_at_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatusWarningType {
    ServiceTree,
    Health,
    Hooks,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStatusView {
    pub id: OperationId,
    pub operation_type: OperationType,
    pub service: String,
    pub source: OperationSource,
    pub state: OperationState,
    pub created_at_ns: u64,
    pub started_at_ns: Option<u64>,
    pub completed_at_ns: Option<u64>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub merged_into: Option<OperationId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    UnknownService { service: String },
    UnknownOperation { operation_id: OperationId },
    MissingCurrentJobRecord { service: String, job_id: JobId },
}
