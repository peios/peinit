use crate::execution::control::{
    ControlExecutionDispatch, ReloadCommandTerminalDispatch, ReloadCommandTimeoutDispatch,
    ReloadDetectionCompletion, StopEscalationDispatch,
};
use crate::execution::launch::LaunchCreatedJobDispatch;

use super::launch::SupervisorPendingProcessSetupDispatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlDispatch {
    pub execution: ControlExecutionDispatch,
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
