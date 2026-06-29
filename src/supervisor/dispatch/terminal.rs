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
