use crate::ids::IdAllocationError;
use crate::operation::OperationSource;
use crate::operation::store::{OperationEvent, OperationRequestOutcome, OperationStoreError};
use crate::service::ServiceDependencyKind;
use crate::service::runtime::TransitionCause;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDemandStartPlan {
    pub requested: String,
    pub requested_operation_source: OperationSource,
    pub requested_transition_cause: TransitionCause,
    pub starts: Vec<PlannedStart>,
    pub blocked: Vec<StartPlanBlockedService>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStart {
    pub service: String,
    pub operation_source: OperationSource,
    pub transition_cause: TransitionCause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPlanBlockedService {
    pub service: String,
    pub reason: StartBlockReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartBlockReason {
    HardDependencyUnavailable {
        target: String,
        kind: ServiceDependencyKind,
        availability: DependencyAvailability,
    },
    HardDependencyBlocked {
        target: String,
        kind: ServiceDependencyKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyAvailability {
    Missing,
    DefinitionRemoved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnDemandStartPlanError {
    UnknownService { service: String },
    DefinitionRemoved { service: String },
    Cycle { services: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDemandStartDispatch {
    pub plan: OnDemandStartPlan,
    pub requested_operation: OperationRequestOutcome,
    pub dependency_operations: Vec<OperationRequestOutcome>,
    pub events: Vec<OperationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnDemandStartDispatchError {
    IdAllocation(IdAllocationError),
    OperationStore(OperationStoreError),
    MissingRequestedStart { service: String },
    MissingDependencyOperationId { service: String },
}
