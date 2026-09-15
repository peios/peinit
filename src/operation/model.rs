use crate::ids::OperationId;
use crate::security::TokenSummary;
use crate::service::ServiceSecurityDescriptor;

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
    /// Where the operation's maximum lifetime (§8.2) is measured from.
    ///
    /// Creation, including queue time — except that a start the graph held
    /// on a fact with no clock of its own (§7.5) had no lifetime while it
    /// was held, so the clock starts again when the hold ends: a start
    /// released after a long hold gets its `StartTimeout` from the
    /// release, not a deadline that expired while it was not allowed to
    /// run (PEI-821). `created_at_ns` stays what it was: when the operator
    /// asked.
    pub lifetime_from_ns: u64,
    pub started_at_ns: Option<u64>,
    pub completed_at_ns: Option<u64>,
    pub source: OperationSource,
    pub caller: Option<TokenSummary>,
    pub result: Option<String>,
    pub merged_into: Option<OperationId>,
    /// The target service's effective `ServiceSecurity` when the operation
    /// was created.
    ///
    /// A terminal operation is retained after its service can have been
    /// discarded — a restart aborted because its definition was withdrawn
    /// mid-stop is the documented case (§8.2) — and `operation-status`
    /// checks `SERVICE_QUERY_STATUS` against the target. With the service
    /// gone there was nothing to check against, and the query answered
    /// `UNKNOWN_SERVICE` for an operation peinit still held (PEI-1076).
    /// Recorded here so the operation stays queryable by exactly whoever
    /// could have queried the service while it existed. `None` only until
    /// the supervisor commits the transaction that created the record.
    pub service_security: Option<ServiceSecurityDescriptor>,
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
