use crate::ids::OperationId;
use crate::security::TokenSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    Start,
    Stop,
    Restart,
    Reload,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationSource {
    Admin,
    Boot,
    Shutdown,
    DependencyPropagation,
    RestartPolicy,
    Timer,
    BindsToRecovery,
    BindsToPropagation,
    ConflictResolution,
    OnFailure,
    /// A service started because the terminal it names in `TTYPath` was
    /// released by whoever held it.
    TtyRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationState {
    Pending,
    Running,
    Completed,
    Failed,
    Merged,
    Cancelled,
    Aborted,
}

impl OperationState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Merged | Self::Cancelled | Self::Aborted
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRecord {
    pub id: OperationId,
    pub operation_type: OperationType,
    pub service: String,
    pub state: OperationState,
    pub created_at_ns: u64,
    pub started_at_ns: Option<u64>,
    pub completed_at_ns: Option<u64>,
    pub source: OperationSource,
    pub caller: Option<TokenSummary>,
    pub result: Option<String>,
    pub merged_into: Option<OperationId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationTransitionError {
    InvalidTransition {
        id: OperationId,
        from: OperationState,
        action: OperationTransitionAction,
    },
    CompletionBeforeCreation {
        id: OperationId,
        created_at_ns: u64,
        completed_at_ns: u64,
    },
    StartBeforeCreation {
        id: OperationId,
        created_at_ns: u64,
        started_at_ns: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationTransitionAction {
    Start,
    Complete,
    Fail,
    Merge,
    Cancel,
    Abort,
}
