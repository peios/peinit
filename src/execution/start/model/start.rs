use crate::execution::graph::{GraphContextId, GraphExecutionEvent, ReadyGraphOperation};
use crate::ids::{JobId, OperationId};
use crate::job::JobEvent;
use crate::operation::store::OperationEvent;
use crate::security::TokenSummary;
use crate::service::{ServiceCheck, ServiceTableTransition};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartExecutionRequest {
    pub ready: ReadyGraphOperation,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub started_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartExecutionDispatch {
    pub ready: ReadyGraphOperation,
    pub job_id: JobId,
    pub operation_event: OperationEvent,
    /// The `Skipped -> Inactive` performed before this start's conditions were
    /// re-evaluated, if the service was Skipped and the start was explicit.
    /// Reported alongside the transition into Starting so the operator sees the
    /// state they knew the service to be in, rather than an unexplained jump.
    pub cleared_skipped: Option<ServiceTableTransition>,
    pub service_transition: ServiceTableTransition,
    pub job_event: JobEvent,
    pub job_kind: StartExecutionJobKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartExecutionOutcome {
    Job(Box<StartExecutionDispatch>),
    Terminal(StartExecutionTerminalDispatch),
    CheckPending(StartExecutionCheckPendingDispatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphPreStartCheckOutcome {
    Passed(GraphPreStartCheckPassedDispatch),
    Terminal(GraphPreStartCheckTerminalDispatch),
    CheckPending(GraphPreStartCheckPendingDispatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPreStartCheckPassedDispatch {
    pub ready: ReadyGraphOperation,
    pub service: String,
    pub operation_id: OperationId,
    pub graph_context_ids: Vec<GraphContextId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPreStartCheckPendingDispatch {
    pub ready: ReadyGraphOperation,
    pub service: String,
    pub operation_id: OperationId,
    pub checks: Vec<ServiceCheck>,
    pub helper_cgroup_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPreStartCheckTerminalDispatch {
    pub ready: ReadyGraphOperation,
    pub outcome: StartPreCheckTerminalOutcome,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartExecutionTerminalDispatch {
    pub ready: ReadyGraphOperation,
    pub outcome: StartPreCheckTerminalOutcome,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartExecutionCheckPendingDispatch {
    pub ready: ReadyGraphOperation,
    pub operation_event: OperationEvent,
    pub service: String,
    pub operation_id: OperationId,
    pub checks: Vec<ServiceCheck>,
    pub helper_cgroup_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPreCheckTerminalOutcome {
    ConditionSkipped {
        check: String,
    },
    /// The service's `TTYPath` was in another service's hands, so it was
    /// skipped rather than started. It goes back into the queue for that
    /// terminal if it carries a `tty:released` trigger.
    TtyUnavailable {
        tty: String,
        holder: String,
    },
    AssertionFailed {
        check: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartExecutionJobKind {
    ServiceMain,
    PreStartHook { main_job_id: JobId },
}
