use crate::execution::graph::GraphExecutionEvent;
use crate::execution::start::{ServiceMainStartTimeoutDispatch, StartExecutionDispatch};
use crate::ids::{JobId, OperationId};
use crate::operation::OperationRecord;
use crate::operation::store::OperationEvent;
use crate::service::RestartWindowResetDeadline;
use crate::supervisor::SupervisorOnFailureLoopSuppressedDispatch;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SupervisorOperationMaintenanceTurn {
    pub operation_timeouts: Vec<OperationEvent>,
    pub service_main_start_timeouts: Vec<ServiceMainStartTimeoutDispatch>,
    pub graph_events: Vec<GraphExecutionEvent>,
    pub relationship_audit_events: Vec<SupervisorOnFailureLoopSuppressedDispatch>,
    pub start_dispatches: Vec<StartExecutionDispatch>,
    pub restart_window_resets: Vec<RestartWindowResetDeadline>,
    pub purged_operations: Vec<OperationId>,
    pub purged_jobs: Vec<crate::ids::JobId>,
}

pub(in crate::supervisor::operation_maintenance) struct PendingOperationTimeout {
    pub operation_events: Vec<OperationEvent>,
    pub graph_events: Vec<GraphExecutionEvent>,
}

pub(in crate::supervisor::operation_maintenance) struct DueOperationMaintenance {
    pub pending_operation_timeouts: Vec<OperationRecord>,
    pub running_service_main_start_timeouts: Vec<RunningServiceMainStartTimeout>,
    pub restart_window_resets: Vec<RestartWindowResetDeadline>,
}

impl DueOperationMaintenance {
    pub fn requires_supervisor_work(&self) -> bool {
        !self.pending_operation_timeouts.is_empty()
            || !self.running_service_main_start_timeouts.is_empty()
            || !self.restart_window_resets.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::supervisor::operation_maintenance) struct RunningServiceMainStartTimeout {
    pub operation_id: OperationId,
    pub job_id: JobId,
}

pub(in crate::supervisor::operation_maintenance) struct RunningServiceMainStartTimeoutDispatch {
    pub timeout: ServiceMainStartTimeoutDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}
