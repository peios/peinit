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

/// Services started because the boot set settled (or the wait timed out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorBootSettleDispatch {
    pub started: Vec<SupervisorBootSettleStart>,
    pub failed: Vec<SupervisorBootSettleFailure>,
    /// The deadline expired rather than the boot set settling — something is
    /// still moving, so a terminal-attached service starting now may still have
    /// its output written over.
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorBootSettleStart {
    pub service: String,
    pub lifecycle: Box<crate::supervisor::dispatch::SupervisorLifecycleDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorBootSettleFailure {
    pub service: String,
    pub error: String,
}
