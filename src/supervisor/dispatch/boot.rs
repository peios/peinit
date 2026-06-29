use crate::boot::phase2::Phase2BootPlan;
use crate::boundary::{BoundaryError, LaunchedFilesystemCheckHelper};
use crate::execution::graph::GraphContextId;
use crate::execution::start::{
    PreStartCheckCompletionDispatch, PreStartCheckTimeoutDispatch, StartExecutionDispatch,
};
use crate::operation::store::Phase2BootDispatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorBootDispatch {
    pub plan: Phase2BootPlan,
    pub operation_dispatch: Phase2BootDispatch,
    pub context_id: GraphContextId,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorFilesystemCheckLaunchDispatch {
    pub helper: LaunchedFilesystemCheckHelper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorFilesystemCheckCompletionDispatch {
    pub completion: PreStartCheckCompletionDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorFilesystemCheckTimeoutDispatch {
    pub timeout: PreStartCheckTimeoutDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorBootSuccessDispatch {
    pub required_critical_services: Vec<String>,
    pub satisfied_since_ns: u64,
    pub due_at_ns: u64,
    pub reset_result: Result<(), BoundaryError>,
}
