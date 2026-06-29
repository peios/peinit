use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::ids::JobId;
use crate::job::JobEvent;
use crate::service::ServiceTableTransition;
use crate::service::runtime::ServiceState;

use super::launch::SupervisorPendingProcessSetupDispatch;
use super::shutdown::SupervisorShutdownFinalizationDispatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorHealthCheckIntervalDispatch {
    pub service: String,
    pub action: SupervisorHealthCheckIntervalAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorHealthCheckIntervalAction {
    Created { job_event: Box<JobEvent> },
    SkippedOverlap { job_id: Option<JobId> },
    SkippedState { state: ServiceState },
    NotConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorHealthCheckLaunchDispatch {
    pub launch: LaunchCreatedJobDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorHealthCheckLaunchResult {
    Launched(SupervisorHealthCheckLaunchDispatch),
    Failed(Box<SupervisorHealthCheckLaunchFailureDispatch>),
    Cancelled(SupervisorHealthCheckLaunchCancelledDispatch),
    PendingSetup(SupervisorPendingProcessSetupDispatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorHealthCheckLaunchFailureDispatch {
    pub terminal: SupervisorHealthCheckTerminalDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorHealthCheckLaunchCancelledDispatch {
    pub job_event: JobEvent,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorHealthCheckTerminalDispatch {
    pub job_event: JobEvent,
    pub service_job_event: Option<JobEvent>,
    pub outcome: SupervisorHealthCheckOutcome,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub killed_cgroup_id: String,
    pub critical_reboot: Option<SupervisorShutdownFinalizationDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorHealthCheckOutcome {
    Healthy,
    Unhealthy {
        consecutive_failures: u32,
        retries: u32,
    },
    RestartScheduled {
        consecutive_failures: u32,
        retries: u32,
    },
    Failed {
        consecutive_failures: u32,
        retries: u32,
    },
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorHealthCheckTimeoutDispatch {
    pub terminal: SupervisorHealthCheckTerminalDispatch,
    pub timed_out_at_ns: u64,
}
