use crate::job::JobEvent;
use crate::service::ServiceTableTransition;

use super::shutdown::SupervisorShutdownFinalizationDispatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorWatchdogNotifyDispatch {
    pub service: String,
    pub generation: u64,
    pub outcome: SupervisorWatchdogNotifyOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorWatchdogNotifyOutcome {
    Armed { due_at_ns: u64 },
    Disabled,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorWatchdogTimeoutDispatch {
    pub service: String,
    pub generation: u64,
    pub job_event: Option<JobEvent>,
    pub outcome: SupervisorWatchdogTimeoutOutcome,
    pub service_transitions: Vec<ServiceTableTransition>,
    pub killed_cgroup_id: Option<String>,
    pub timed_out_at_ns: u64,
    pub critical_reboot: Option<SupervisorShutdownFinalizationDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorWatchdogTimeoutOutcome {
    RestartScheduled,
    Failed,
    Stale,
}
