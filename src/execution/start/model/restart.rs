use crate::ids::{JobId, OperationId};
use crate::job::JobEvent;
use crate::operation::store::OperationEvent;
use crate::security::TokenSummary;
use crate::service::{ServiceCheck, ServiceTableTransition};

use super::start::{StartExecutionJobKind, StartPreCheckTerminalOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartStartExecutionRequest {
    pub service: String,
    pub operation_id: OperationId,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub started_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartStartExecutionDispatch {
    pub service: String,
    pub operation_id: OperationId,
    pub job_id: JobId,
    pub service_transition: ServiceTableTransition,
    pub job_event: JobEvent,
    pub job_kind: StartExecutionJobKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartStartExecutionOutcome {
    Job(Box<RestartStartExecutionDispatch>),
    Terminal(RestartStartExecutionTerminalDispatch),
    CheckPending(RestartStartExecutionCheckPendingDispatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartStartExecutionTerminalDispatch {
    pub service: String,
    pub operation_id: OperationId,
    pub outcome: StartPreCheckTerminalOutcome,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    /// Graph events from the failure, where the restart's operation belongs to
    /// a context. An administrator's restart creates none, so this is usually
    /// empty; a restart-policy relaunch does, and its co-released members are
    /// what the propagation reaches.
    pub graph_events: Vec<crate::execution::graph::GraphExecutionEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartStartExecutionCheckPendingDispatch {
    pub service: String,
    pub operation_id: OperationId,
    pub service_transition: ServiceTableTransition,
    pub checks: Vec<ServiceCheck>,
    pub helper_cgroup_id: String,
}
