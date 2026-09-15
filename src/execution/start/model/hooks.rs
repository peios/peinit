use crate::execution::graph::{GraphContextId, GraphExecutionEvent};
use crate::ids::JobId;
use crate::job::JobEvent;
use crate::operation::store::OperationEvent;
use crate::service::ServiceTableTransition;

use super::start::StartExecutionJobKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreStartHookTerminalDispatch {
    pub job_event: JobEvent,
    pub next_job_event: Option<JobEvent>,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub killed_cgroup_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreStartCheckCompletionDispatch {
    pub result_fd: i32,
    pub job_id: Option<JobId>,
    pub job_event: Option<JobEvent>,
    pub job_kind: Option<StartExecutionJobKind>,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub graph_context_ids: Vec<GraphContextId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreStartCheckTimeoutDispatch {
    pub completion: PreStartCheckCompletionDispatch,
    pub pidfd: i32,
    pub killed_cgroup_id: String,
    pub timed_out_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreStartHookTimeoutDispatch {
    pub job_event: JobEvent,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub killed_cgroup_id: String,
    pub timed_out_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostStartHookTerminalDispatch {
    pub job_event: JobEvent,
    pub next_job_event: Option<JobEvent>,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub killed_cgroup_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostStartHookTimeoutDispatch {
    pub job_event: JobEvent,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub killed_cgroup_id: String,
    pub timed_out_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessTimeoutDispatch {
    /// The main job the timeout killed, failed here so its later exit is not
    /// evaluated as a second failure (PEI-822). `None` when the job was not
    /// yet running at the deadline.
    pub job_event: Option<JobEvent>,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub killed_cgroup_id: String,
    pub timed_out_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceMainStartTimeoutDispatch {
    pub job_event: JobEvent,
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub killed_cgroup_id: Option<String>,
    pub timed_out_at_ns: u64,
}
