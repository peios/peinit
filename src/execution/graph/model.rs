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
    /// The start operation failed but the service went to Backoff: it is
    /// *going* to start again, under an operation this context does not
    /// own. Not terminal, so the context stays live and every dependent
    /// waiting on this member stays held (§6.1) — a held dependent looks
    /// exactly like a start held on a readiness level, Inactive with its
    /// operation Pending. The hold ends when the service next reaches a
    /// dependent-satisfying state (`Satisfied`) or gives up — restart
    /// budget exhausted, stopped, withdrawn — (`Failed`), through
    /// `settle_awaiting_restart` (PEI-821).
    AwaitingRestart,
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
    /// Whether the last look at this member, while it waited, found it
    /// held only on facts with no clock (§7.5, `waits_without_clock`).
    /// Read when it is released: such a start's lifetime runs from the
    /// release, since it had none while held (PEI-821).
    pub held_without_clock: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphDependency {
    pub dependent: String,
    pub target: String,
    /// The readiness level the target must have published, when the
    /// declaration named one (`Requires = ["netd:routed"]`).
    ///
    /// A level edge is the one kind of edge whose target may not be a
    /// member of the context: an already-active target is excluded from
    /// the start plan (there is nothing to start), but the *condition* on
    /// it still has to be waited for. Level-less edges to non-members stay
    /// excluded, as they always were.
    pub level: Option<String>,
    pub kind: ServiceDependencyKind,
}

/// What a live look at a level dependency's target found.
///
/// Levels are claims made by a running process over the notify socket, so
/// they cannot be settled from the graph's own bookkeeping the way member
/// completion can: the answer has to come from the service table at the
/// moment of the check. `release_ready` takes a probe returning this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelProbe {
    /// Running, and has published exactly the wanted level.
    Satisfied,
    /// Running, but its current level is different or not yet published.
    /// The wanted level may still arrive.
    NotYetPublished,
    /// Not running: stopped, disabled, or gone. Nothing is making claims.
    Absent,
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

    /// Every launch this context planned has been decided against: each
    /// member is terminal, is awaiting a restart (PEI-821), or is held only
    /// on members that are. The boot window (§3.7) closes on this — plus
    /// the launched members the supervisor knows about — rather than on
    /// `is_drained`: a boot service in Backoff and the dependents held for
    /// it are the restart policy's business, and holding every reload for
    /// the length of a retry cycle would be the cost of a crash loop, not
    /// of the boot.
    pub fn has_attempted_every_launch(&self) -> bool {
        self.settled_launches().len() == self.members.len()
    }

    /// Whether this member's launch has been attempted or decided against
    /// — the per-member half of [`Self::has_attempted_every_launch`].
    ///
    /// A member that has not is the one thing a reload during the boot
    /// window must not touch: its start is still to be made from the
    /// boot's snapshot (§3.7, PEI-350). A name that is not a member counts
    /// as attempted; the context has nothing planned for it.
    pub fn launch_attempted(&self, service: &str) -> bool {
        !self.members.contains_key(service) || self.settled_launches().contains(service)
    }

    /// The members whose launch has not been attempted, in plan order.
    pub fn unattempted_launches(&self) -> Vec<String> {
        let settled = self.settled_launches();
        let mut members = self
            .members
            .values()
            .filter(|member| !settled.contains(member.service.as_str()))
            .collect::<Vec<_>>();
        members.sort_by_key(|member| member.sequence);
        members
            .into_iter()
            .map(|member| member.service.clone())
            .collect()
    }

    /// The members that are terminal, awaiting a restart, or held only on
    /// members that are — closed over the dependency edges, so a dependent
    /// waiting on nothing that can still launch is settled too.
    ///
    /// A `Running` member is dispatched, not necessarily launched: its job
    /// may still be queued, and its activation snapshot is taken at the
    /// launch. Whether it has launched is the job store's knowledge, so the
    /// supervisor adds that half (`boot_window.rs`); here it stays
    /// unsettled, and so does anything held on it.
    fn settled_launches(&self) -> std::collections::BTreeSet<&str> {
        let mut decided: std::collections::BTreeSet<&str> = self
            .members
            .values()
            .filter(|member| {
                member.status.is_terminal() || member.status == GraphMemberStatus::AwaitingRestart
            })
            .map(|member| member.service.as_str())
            .collect();
        loop {
            let mut grew = false;
            for member in self.members.values() {
                if decided.contains(member.service.as_str()) {
                    continue;
                }
                let held = matches!(
                    member.status,
                    GraphMemberStatus::Dormant | GraphMemberStatus::WaitingForDependencies
                );
                let waits_only_on_decided = held
                    && self
                        .dependencies
                        .iter()
                        .filter(|edge| edge.dependent == member.service)
                        .filter(|edge| self.members.contains_key(&edge.target))
                        .all(|edge| decided.contains(edge.target.as_str()));
                if waits_only_on_decided {
                    decided.insert(member.service.as_str());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        decided
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
    /// The start was held on facts with no clock (§7.5) and is released
    /// now: its operation lifetime starts here (PEI-821).
    pub released_from_hold: bool,
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
