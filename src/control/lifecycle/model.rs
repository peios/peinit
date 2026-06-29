use crate::ids::OperationId;
use crate::operation::OperationType;
use crate::operation::store::{OperationEvent, OperationRequestOutcome, OperationStoreError};
use crate::security::TokenSummary;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::service::{ServiceTableError, ServiceTableTransition};

use super::start_plan::{
    OnDemandStartDispatch, OnDemandStartDispatchError, OnDemandStartPlanError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleCommand {
    Start,
    Stop,
    Restart,
    Reload,
    Reset,
}

impl LifecycleCommand {
    pub fn operation_type(self) -> OperationType {
        match self {
            Self::Start => OperationType::Start,
            Self::Stop => OperationType::Stop,
            Self::Restart => OperationType::Restart,
            Self::Reload => OperationType::Reload,
            Self::Reset => OperationType::Reset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleCommandRequest {
    pub id: OperationId,
    pub command: LifecycleCommand,
    pub service: String,
    pub caller: Option<TokenSummary>,
    pub created_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatusSnapshot {
    pub service: String,
    pub state: ServiceState,
    pub cause: Option<TransitionCause>,
    pub generation: u64,
    pub definition_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleCommandOutcome {
    OperationAccepted(OperationRequestOutcome),
    OnDemandStart(OnDemandStartDispatch),
    Already(ServiceStatusSnapshot),
    Noop(ServiceStatusSnapshot),
    SynchronousClear(Box<SynchronousClearOutcome>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynchronousClearOutcome {
    pub request: OperationRequestOutcome,
    pub started: OperationEvent,
    pub service_transition: ServiceTableTransition,
    pub completed: OperationEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleCommandError {
    UnknownService {
        service: String,
    },
    DefinitionRemoved {
        service: String,
    },
    InvalidState {
        service: String,
        command: LifecycleCommand,
        state: ServiceState,
    },
    ExpectedMerge {
        service: String,
        command: LifecycleCommand,
    },
    ExpectedQueue {
        service: String,
        command: LifecycleCommand,
    },
    StartPlan(OnDemandStartPlanError),
    StartDispatch(OnDemandStartDispatchError),
    OperationStore(OperationStoreError),
    ServiceTable(ServiceTableError),
}
