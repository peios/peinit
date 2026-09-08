use crate::boot::BootMode;
use crate::ids::{IdAllocationError, JobId, OperationId};
pub use crate::service::ServiceDependencyKind as DependencyKind;
use crate::service::ServiceGraphWarning;
use crate::service::runtime::TransitionCause;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartCause {
    ExplicitStart,
    DependencyStart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockedReason {
    HardDependencyUnavailable {
        target: String,
        kind: DependencyKind,
    },
    HardDependencyBlocked {
        target: String,
        kind: DependencyKind,
    },
    CycleDetected {
        services: Vec<String>,
    },
    ConflictingBootService {
        target: String,
    },
    ValidationError {
        message: String,
    },
}

impl BlockedReason {
    pub fn transition_cause(&self) -> TransitionCause {
        match self {
            Self::HardDependencyUnavailable { .. } | Self::HardDependencyBlocked { .. } => {
                TransitionCause::DependencyFailure
            }
            Self::CycleDetected { .. } => TransitionCause::CycleDetected,
            Self::ConflictingBootService { .. } | Self::ValidationError { .. } => {
                TransitionCause::ValidationError
            }
        }
    }
}

/// A finding that forced a Full boot down to Safe mode.
///
/// Boot-level rather than per-service, deliberately. The downgrade rebuilds
/// the graph in Safe mode and discards the Full-mode one, so the services
/// named here are never entered into `blocked` and never marked Failed —
/// Safe mode was never going to start them, and a Failed state would say
/// something about their own health that is not true. This records why the
/// machine came up in Safe mode without making that claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafeModeDowngrade {
    CriticalCycle { services: Vec<String> },
    CriticalBootConflict { service: String, target: String },
}

impl core::fmt::Display for SafeModeDowngrade {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CriticalCycle { services } => write!(
                formatter,
                "critical service in dependency cycle {}",
                services.join(" -> "),
            ),
            Self::CriticalBootConflict { service, target } => write!(
                formatter,
                "critical boot-triggered services {service} and {target} conflict",
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedService {
    pub service: String,
    pub operation_id: OperationId,
    /// The primary Failed cause, by PSD-007 §6.2 precedence. This is what
    /// `transition_cause()` turns into the service's recorded state.
    pub reason: BlockedReason,
    /// Every other finding for this service, in discovery order. §6.2 requires
    /// these be logged; they deliberately do not affect the primary cause.
    pub additional_reasons: Vec<BlockedReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedStart {
    pub service: String,
    pub operation_id: OperationId,
    pub job_id: JobId,
    pub cause: StartCause,
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase2BootPlan {
    pub mode: BootMode,
    pub observed_at_ns: u64,
    pub max_parallel_starts: u32,
    pub starts: Vec<PreparedStart>,
    pub blocked: Vec<BlockedService>,
    /// Why this boot was downgraded to Safe mode; empty if it was not.
    pub safe_mode_downgrade: Vec<SafeModeDowngrade>,
    /// Graph validation warnings for the definition set this plan was
    /// built from — an `Alive` service with hard dependents, a role
    /// nothing fills.
    ///
    /// Warnings are not blocking, and the plan is built whether there
    /// are any or not. They are carried here so the boot can emit them,
    /// which is the boot where they matter: a `Readiness=Alive` service
    /// with dependents is about to release them before it is usable, and
    /// an operator who only ever saw that on a later `reload-config`
    /// would be told after the fact.
    pub warnings: Vec<ServiceGraphWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase2BootPlanError {
    InvalidMaxParallelStarts,
    DuplicateService { service: String },
    MissingServiceDefinition { service: String },
    Cycle { services: Vec<String> },
    OperationIdAllocation(IdAllocationError),
    JobIdAllocation(IdAllocationError),
}
