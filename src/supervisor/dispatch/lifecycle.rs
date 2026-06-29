use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::execution::graph::GraphContextId;
use crate::execution::start::StartExecutionDispatch;

use super::super::control_boundary::PendingControlOperation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLifecycleDispatch {
    pub outcome: LifecycleCommandOutcome,
    pub context_id: Option<GraphContextId>,
    pub start_dispatches: Vec<StartExecutionDispatch>,
    pub pending_control_operation: Option<PendingControlOperation>,
    pub lifecycle_warnings: Vec<String>,
}
