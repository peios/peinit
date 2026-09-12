use crate::execution::control::{
    ControlExecutionDispatch, ControlExecutionError, ReloadCommandTerminalDispatch,
    ReloadCommandTimeoutDispatch, ReloadDetectionCompletion, StopEscalationDispatch,
};
use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::ids::OperationId;
use crate::operation::OperationType;
use crate::operation::store::OperationEvent;

use super::launch::SupervisorPendingProcessSetupDispatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlDispatch {
    pub execution: ControlExecutionDispatch,
}

/// A control operation the boundary refused to begin.
///
/// The operation was admitted and then execution found nothing to act on —
/// no current main job, a transition the state machine does not permit. That
/// is a fault in peinit's own bookkeeping, and until PEI-803 it left the
/// work pump as a fatal loop error: PID 1 entered recovery and unlinked its
/// sockets over one service's operation. Now the operation fails with an
/// `internal_error` result, the service keeps the state it had, and this
/// dispatch carries the evidence to the console and the event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlFailureDispatch {
    pub operation_id: OperationId,
    pub service: String,
    pub operation_type: OperationType,
    pub operation_event: OperationEvent,
    pub error: ControlExecutionError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlLaunchDispatch {
    pub launch: LaunchCreatedJobDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorControlLaunchResult {
    Launched(Box<SupervisorControlLaunchDispatch>),
    PendingSetup(SupervisorPendingProcessSetupDispatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorStopEscalationDispatch {
    pub escalation: StopEscalationDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorReloadDetectionDispatch {
    pub completion: ReloadDetectionCompletion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorReloadCommandTerminalDispatch {
    pub terminal: ReloadCommandTerminalDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorReloadCommandTimeoutDispatch {
    pub timeout: ReloadCommandTimeoutDispatch,
}
