use crate::boundary::ProcessController;
use crate::execution::control::ControlExecutionStore;
use crate::execution::graph::GraphExecutionEvent;
use crate::execution::graph::GraphExecutionStore;
use crate::execution::start::{StartExecutionError, StartExecutionStore};
use crate::ids::{JobId, JobIdAllocator, OperationId};
use crate::job::{JobState, JobStore};
use crate::operation::OperationType;
use crate::operation::store::{OperationEvent, OperationStore, OperationStoreError};
use crate::service::runtime::ServiceState;
use crate::service::{ServiceTable, ServiceTableError, ServiceTableTransition};

pub struct NotifyApplyContext<'a, P>
where
    P: ProcessController + ?Sized,
{
    pub services: &'a mut ServiceTable,
    pub operations: &'a mut OperationStore,
    pub graph: &'a mut GraphExecutionStore,
    pub jobs: &'a mut JobStore,
    pub job_ids: &'a mut JobIdAllocator,
    pub start_store: &'a mut StartExecutionStore,
    pub control_store: &'a mut ControlExecutionStore,
    pub controller: &'a mut P,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyApplyRequest {
    pub sender_pid: u32,
    pub observed_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedNotifySender {
    pub service: String,
    pub job_id: JobId,
    pub operation_id: Option<OperationId>,
    pub generation: u64,
    pub job_created_at_ns: u64,
    pub cgroup_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyApplyDispatch {
    pub sender: AuthenticatedNotifySender,
    pub applied_fields: Vec<NotifyAppliedField>,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub post_start_hook: Option<crate::job::JobEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyAppliedField {
    Ready,
    Reloading,
    Stopping,
    Status { text: String },
    /// A readiness level the service published, for dependents declaring
    /// `Requires = ["<service>:<level>"]`. An empty value retracts it.
    Level { value: String },
    Progress { value: String },
    ProgressUnit { value: String },
    Errno { value: String },
    ExitStatus { value: String },
    Watchdog,
    WatchdogUsec { value: String },
    ExtendTimeoutUsec { value: String },
    FdStore,
    FdName { name: String },
    FdStoreRemove,
    FdPoll { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyApplyError {
    UnauthenticatedSender {
        pid: u32,
    },
    MissingService {
        job_id: JobId,
    },
    JobNotRunning {
        job_id: JobId,
        state: JobState,
    },
    MissingProcess {
        job_id: JobId,
    },
    PidfdMismatch {
        job_id: JobId,
        pid: u32,
        pidfd: i32,
    },
    ProcessVerification {
        job_id: JobId,
        pid: u32,
        pidfd: i32,
        message: String,
    },
    GenerationMismatch {
        service: String,
        job_generation: u64,
        runtime_generation: u64,
    },
    MissingStartOperation {
        service: String,
        job_id: JobId,
    },
    MissingReloadOperation {
        service: String,
    },
    UnsupportedReloadOperation {
        service: String,
        operation_type: OperationType,
    },
    UnsupportedReadyState {
        service: String,
        state: ServiceState,
    },
    ServiceTable(ServiceTableError),
    OperationStore(OperationStoreError),
    Start(StartExecutionError),
}
