use std::collections::BTreeMap;

use crate::ids::{JobId, OperationId};
use crate::service::ServiceDependencyKind;
use crate::service::runtime::TransitionCause;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphContextId(pub(super) u64);

impl GraphContextId {
    pub fn as_u64(self) -> u64 {
        self.0
    }

    #[cfg(all(test, feature = "peios-boundary"))]
    pub(crate) fn new_for_test(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphContextKind {
    Boot,
    OnDemand {
        requested_service: String,
        requested_operation_id: OperationId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphMemberStatus {
    Dormant,
    WaitingForPreStartCheck,
    WaitingForDependencies,
    Running,
    Satisfied,
    Failed,
    Pruned,
}

impl GraphMemberStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Satisfied | Self::Failed | Self::Pruned)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMember {
    pub service: String,
    pub operation_id: OperationId,
    pub reserved_job_id: Option<JobId>,
    pub transition_cause: TransitionCause,
    pub sequence: usize,
    pub status: GraphMemberStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphDependency {
    pub dependent: String,
    pub target: String,
    pub kind: ServiceDependencyKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphExecutionContext {
    pub id: GraphContextId,
    pub kind: GraphContextKind,
    pub members: BTreeMap<String, GraphMember>,
    pub dependencies: Vec<GraphDependency>,
}

impl GraphExecutionContext {
    pub fn member_for_operation(&self, operation_id: OperationId) -> Option<&GraphMember> {
        self.members
            .values()
            .find(|member| member.operation_id == operation_id)
    }

    pub fn is_drained(&self) -> bool {
        self.members
            .values()
            .all(|member| member.status.is_terminal())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphTerminalOutcome {
    Satisfied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphExecutionEvent {
    pub context_id: GraphContextId,
    pub service: String,
    pub operation_id: OperationId,
    pub outcome: GraphTerminalOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPrunedOperation {
    pub context_id: GraphContextId,
    pub service: String,
    pub operation_id: OperationId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyGraphOperation {
    pub context_id: GraphContextId,
    pub service: String,
    pub operation_id: OperationId,
    pub reserved_job_id: Option<JobId>,
    pub transition_cause: TransitionCause,
    pub action: ReadyGraphOperationAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyGraphOperationAction {
    PreStartCheck,
    Start,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphContextBuildError {
    MissingServiceDefinition { service: String },
    DuplicateServiceMember { service: String },
    MissingDependencyOperationOutcome { service: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphExecutionError {
    UnknownContext {
        context_id: GraphContextId,
    },
    InvalidMaxParallelStarts,
    UnknownAssociatedOperation {
        operation_id: OperationId,
    },
    MissingMember {
        context_id: GraphContextId,
        service: String,
    },
    MemberAlreadyTerminal {
        context_id: GraphContextId,
        service: String,
        status: GraphMemberStatus,
    },
}
