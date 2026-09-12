use crate::execution::job_terminal::ServiceMainJobTerminalDispatch;
use crate::execution::restart_policy::RestartPolicyRelaunchDispatch;
use crate::execution::start::{RestartStartExecutionDispatch, StartExecutionDispatch};
use crate::job::JobEvent;
use crate::service::RestartBackoffDeadline;

use super::shutdown::SupervisorShutdownFinalizationDispatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorTerminalDispatch {
    pub terminal: ServiceMainJobTerminalDispatch,
    pub cleanup_job_events: Vec<JobEvent>,
    pub start_dispatches: Vec<StartExecutionDispatch>,
    pub restart_start_dispatches: Vec<RestartStartExecutionDispatch>,
    pub critical_reboot: Option<SupervisorShutdownFinalizationDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorRestartBackoffDispatch {
    pub due: RestartBackoffDeadline,
    pub relaunch: RestartPolicyRelaunchDispatch,
}

/// A due restart that peinit could not execute.
///
/// The relaunch was refused by peinit's own bookkeeping — an operation in a
/// shape the deadline path does not expect, a plan it cannot build. That is
/// not the service's doing, and until PEI-808 it ended the runtime loop over
/// one service's restart. Now the service goes `Backoff -> Failed` under
/// `InternalError`, any operation waiting on the restart fails with the
/// `internal_error` result, and this dispatch carries the evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorRestartBackoffFailureDispatch {
    pub due: RestartBackoffDeadline,
    pub service_transition: crate::service::ServiceTableTransition,
    pub operation_event: Option<crate::operation::store::OperationEvent>,
    pub error: crate::execution::restart_policy::RestartPolicyRelaunchError,
}
