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
    /// Process setups of the Starting services' cancelled jobs, for the
    /// runtime to unregister and close. The supervisor has dropped them; the
    /// descriptors are registered with epoll and only the runtime can take
    /// them out.
    pub cancelled_setups: Vec<SupervisorCancelledProcessSetupDispatch>,
}

/// A launched process whose job the shutdown cancelled before its setup
/// status was read (PEI-826).
///
/// Its job record is gone, so a status arriving later would find nothing to
/// apply it to; the setup is dropped with the job, and the runtime removes the
/// descriptor from epoll so no such status is ever read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorCancelledProcessSetupDispatch {
    pub job_id: crate::ids::JobId,
    pub service: String,
    pub setup_status_fd: i32,
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
    /// Set when an already-stopping participant's retained timeout evidence
    /// could not substantiate a deadline, so it was given none: the deadline
    /// above is already due and the timeout scan escalates it to SIGKILL. The
    /// text says which way the evidence was unusable.
    pub unsubstantiated_deadline: Option<&'static str>,
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
    /// The final action, where the caller asked for it to be attempted at
    /// once; `None` when it was left to the end of the runtime's turn.
    pub finalization: Option<SupervisorShutdownFinalizationDispatch>,
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
