use crate::boundary::{BoundaryError, ProcessController, ProcessSignal, ProcessTarget};
use crate::ids::{IdAllocationError, JobId, JobIdAllocator, OperationId};
use crate::job::{JobEvent, JobState, JobStore, JobStoreError, JobType};
use crate::operation::OperationType;
use crate::operation::store::{OperationEvent, OperationStore, OperationStoreError};
use crate::service::{ServiceTable, ServiceTableError, ServiceTableTransition};

use super::super::command::ExecutableCommandParseError;
use super::store::{ControlExecutionStore, ReloadDetectionPhase};

pub struct ControlExecutionContext<'a, P>
where
    P: ProcessController + ?Sized,
{
    pub services: &'a mut ServiceTable,
    pub operations: &'a mut OperationStore,
    pub jobs: &'a mut JobStore,
    pub job_ids: &'a mut JobIdAllocator,
    pub control_store: &'a mut ControlExecutionStore,
    pub controller: &'a mut P,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlOperationRequest {
    pub operation_id: OperationId,
    pub observed_at_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOperationKind {
    Stop,
    RestartStopLeg,
    ReloadSignal,
    ReloadCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlExecutionDispatch {
    pub operation_id: OperationId,
    pub service: String,
    pub kind: ControlOperationKind,
    pub operation_event: OperationEvent,
    /// `None` when the operation adopted a stop leg already in flight -- a
    /// stop that aborted a running restart (§8.3) -- and the service was
    /// therefore already Stopping (PEI-824).
    pub service_transition: Option<ServiceTableTransition>,
    pub detail: ControlExecutionDetail,
    pub deadline_ns: u64,
    /// Reload command jobs this stop cancelled, already failed in the job
    /// store so their terminals never route as reload completions against a
    /// service that is no longer Reloading (PEI-824).
    pub cancelled_reload_jobs: Vec<JobEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlExecutionDetail {
    Signal {
        target: ProcessTarget,
        signal: ProcessSignal,
    },
    StopAlreadyAcknowledged {
        target: ProcessTarget,
    },
    ReloadCommand {
        job_id: JobId,
        job_event: Box<JobEvent>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopEscalationDispatch {
    pub operation_id: OperationId,
    pub service: String,
    pub cgroup_id: String,
    pub escalated_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadDetectionCompletion {
    pub operation_event: OperationEvent,
    pub service_transition: ServiceTableTransition,
    /// Which of the two advisory outcomes this was.
    ///
    /// `ExtendedWait` is the diagnostic one: the service explicitly announced
    /// `RELOADING=1` and then never said `READY=1`, so it has either wedged
    /// mid-reload or lost its handler. That is worse than a service which
    /// never implements the handshake at all, because this one started
    /// something. Carried here because the phase was otherwise lost at the
    /// dispatch boundary, and `reload` defaults to `wait=false` — so the
    /// default way to issue a reload produced no record of it anywhere
    /// (PEI-359).
    pub phase: ReloadDetectionPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadCommandTerminalDispatch {
    pub job_event: JobEvent,
    pub operation_event: OperationEvent,
    pub service_transition: ServiceTableTransition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadCommandTimeoutDispatch {
    pub job_event: JobEvent,
    pub operation_event: OperationEvent,
    pub service_transition: ServiceTableTransition,
    pub cgroup_id: String,
    pub timed_out_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlExecutionError {
    OperationStore(OperationStoreError),
    ServiceTable(ServiceTableError),
    JobIdAllocation(IdAllocationError),
    JobStore(JobStoreError),
    HookJob(crate::job::ServiceHookJobBuildError),
    Boundary(BoundaryError),
    UnsupportedOperationType {
        operation_id: OperationId,
        operation_type: OperationType,
    },
    MissingCurrentMainJob {
        service: String,
    },
    MissingJobRecord {
        job_id: JobId,
    },
    JobNotRunning {
        job_id: JobId,
    },
    MissingProcessHandle {
        job_id: JobId,
    },
    NotReloadHookJob {
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
    NotTerminalJobEvent {
        job_id: JobId,
        state: JobState,
    },
    InvalidExecReloadCommand {
        service: String,
        exec_reload: String,
        source: ExecutableCommandParseError,
    },
    InvalidReloadSignal {
        service: String,
        signal: String,
    },
}
