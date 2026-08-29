use crate::boundary::{ProcessSignal, ProcessTarget};
use crate::control::system::SystemShutdownCommandOutcome;
use crate::job::JobEvent;
use crate::operation::store::OperationEvent;
use crate::service::ServiceTableTransition;
use crate::shutdown::{
    ShutdownFinalizationReport, ShutdownFinalizationState, ShutdownKind, ShutdownRuntime,
    ShutdownSignal, ShutdownStopDeadline,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownDispatch {
    pub runtime: ShutdownRuntime,
    pub completed_transitions: Vec<ServiceTableTransition>,
    pub killed_starting: Vec<SupervisorShutdownKillDispatch>,
    pub first_wave: Vec<SupervisorShutdownStopDispatch>,
    pub startup_operation_events: Vec<OperationEvent>,
    pub startup_job_events: Vec<JobEvent>,
    /// Every live submitted job, signalled to stop at once (PSPU §7.10).
    pub submitted_stops: Vec<super::submitted::SupervisorSubmittedStopDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSystemShutdownDispatch {
    pub command: SystemShutdownCommandOutcome,
    pub shutdown: SupervisorShutdownDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownKillDispatch {
    pub service: String,
    pub cgroup_id: String,
    pub service_transition: ServiceTableTransition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownStopDispatch {
    pub service: String,
    pub already_stopping: bool,
    pub target: Option<ProcessTarget>,
    pub signal: Option<ProcessSignal>,
    pub service_transition: Option<ServiceTableTransition>,
    pub deadline: Option<ShutdownStopDeadline>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownTerminalDispatch {
    pub job_event: crate::job::JobEvent,
    pub service_transition: Option<ServiceTableTransition>,
    pub next_wave: Vec<SupervisorShutdownStopDispatch>,
    pub finalization: ShutdownFinalizationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownTimeoutDispatch {
    pub global_timeout: bool,
    pub cgroup_kills: Vec<SupervisorShutdownCgroupKillDispatch>,
    pub job_events: Vec<JobEvent>,
    pub abandoned: Vec<SupervisorShutdownAbandonedDispatch>,
    pub next_wave: Vec<SupervisorShutdownStopDispatch>,
    pub finalization: ShutdownFinalizationState,
    pub submitted: Vec<super::submitted::SupervisorSubmittedDeadlineDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownCgroupKillDispatch {
    pub service: String,
    pub cgroup_id: String,
    pub killed_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownAbandonedDispatch {
    pub service: String,
    pub cgroup_id: String,
    pub service_transition: ServiceTableTransition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownFinalizationDispatch {
    pub report: ShutdownFinalizationReport,
    pub finalization: ShutdownFinalizationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorImmediateShutdownDispatch {
    pub killed_services: Vec<SupervisorShutdownCgroupKillDispatch>,
    pub finalization: SupervisorShutdownFinalizationDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownSignalDispatch {
    pub signal: ShutdownSignal,
    pub action: SupervisorShutdownSignalAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorShutdownSignalAction {
    Graceful(SupervisorShutdownDispatch),
    Forced(SupervisorImmediateShutdownDispatch),
    AlreadyInProgress { kind: ShutdownKind },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPowerButtonDispatch {
    pub action: SupervisorPowerButtonAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorPowerButtonAction {
    Graceful(Box<SupervisorShutdownDispatch>),
    AlreadyInProgress { kind: ShutdownKind },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownDriveDispatch {
    pub timeout: Option<SupervisorShutdownTimeoutDispatch>,
    pub finalization: Option<SupervisorShutdownFinalizationDispatch>,
}
