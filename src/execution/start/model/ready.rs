use crate::execution::graph::GraphExecutionEvent;
use crate::ids::{JobId, OperationId};
use crate::job::JobEvent;
use crate::operation::store::OperationEvent;
use crate::service::ServiceTableTransition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartReadyRequest {
    pub service: String,
    pub operation_id: OperationId,
    pub job_id: JobId,
    pub job_created_at_ns: u64,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub ready_at_ns: u64,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartReadyDispatch {
    pub operation_events: Vec<OperationEvent>,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub post_start_hook: Option<JobEvent>,
}
