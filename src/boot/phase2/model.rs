use crate::boot::BootMode;
use crate::ids::{IdAllocationError, JobId, OperationId};
pub use crate::service::ServiceDependencyKind as DependencyKind;
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
