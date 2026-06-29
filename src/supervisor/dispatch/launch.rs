use crate::execution::failure::StartFailureDispatch;
use crate::execution::job_started::ServiceMainJobStartedDispatch;
use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::execution::start::{PostStartHookTerminalDispatch, StartExecutionDispatch};
use crate::job::{JobEvent, JobType};

use super::control::{SupervisorControlLaunchDispatch, SupervisorReloadCommandTimeoutDispatch};
use super::health::{
    SupervisorHealthCheckLaunchDispatch, SupervisorHealthCheckLaunchFailureDispatch,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPendingProcessSetupDispatch {
    pub job_id: crate::ids::JobId,
    pub job_type: JobType,
    pub setup_status_fd: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorProcessSetupDispatch {
    Pending(SupervisorPendingProcessSetupDispatch),
    Stale { setup_status_fd: i32 },
    ServiceMainLaunched(Box<SupervisorLaunchDispatch>),
    ServiceMainFailed(Box<SupervisorLaunchFailureDispatch>),
    StartHookLaunched(SupervisorStartHookLaunchDispatch),
    StartHookFailed(Box<SupervisorStartHookLaunchFailureDispatch>),
    PostHookLaunched(Box<SupervisorPostStartHookLaunchDispatch>),
    PostHookFailed(Box<SupervisorPostStartHookLaunchFailureDispatch>),
    ControlLaunched(SupervisorControlLaunchDispatch),
    ControlTimeout(Box<SupervisorReloadCommandTimeoutDispatch>),
    HealthCheckLaunched(SupervisorHealthCheckLaunchDispatch),
    HealthCheckFailed(Box<SupervisorHealthCheckLaunchFailureDispatch>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLaunchDispatch {
    pub launch: LaunchCreatedJobDispatch,
    pub started: ServiceMainJobStartedDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorServiceLaunchDispatch {
    Launched(Box<SupervisorLaunchDispatch>),
    Failed(Box<SupervisorLaunchFailureDispatch>),
    PendingSetup(SupervisorPendingProcessSetupDispatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLaunchFailureDispatch {
    pub job_event: JobEvent,
    pub failure: StartFailureDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorStartHookLaunchDispatch {
    pub launch: LaunchCreatedJobDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorStartHookLaunchResult {
    Launched(SupervisorStartHookLaunchDispatch),
    Failed(SupervisorStartHookLaunchFailureDispatch),
    PendingSetup(SupervisorPendingProcessSetupDispatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorStartHookLaunchFailureDispatch {
    pub job_event: JobEvent,
    pub failure: StartFailureDispatch,
    pub killed_cgroup_id: String,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPostStartHookLaunchDispatch {
    pub launch: LaunchCreatedJobDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorPostStartHookLaunchResult {
    Launched(Box<SupervisorPostStartHookLaunchDispatch>),
    Failed(Box<SupervisorPostStartHookLaunchFailureDispatch>),
    PendingSetup(SupervisorPendingProcessSetupDispatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPostStartHookLaunchFailureDispatch {
    pub terminal: PostStartHookTerminalDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}
