//! What it means for a start to be *held*.
//!
//! A member the scheduler has not yet released is waiting on something —
//! a pre-start check, a dependency, a readiness level — and its start
//! operation is Pending for as long as that is undecided. §7.5 names the
//! consequence: a held start does not time out. It is the declared
//! semantics, not a hang; the condition was never met, so the start never
//! happened. The operation lifetime of §8.2 is for an operation that is
//! queued behind another, and the maintenance sweep asks here before
//! applying it (PEI-830).

use crate::ids::OperationId;

use super::model::GraphMemberStatus;
use super::store::GraphExecutionStore;

impl GraphMemberStatus {
    /// Still in the scheduler's hands: not yet released to start, and not
    /// finished. A Pending operation on such a member is a held start.
    pub fn is_held(self) -> bool {
        matches!(
            self,
            Self::Dormant | Self::WaitingForPreStartCheck | Self::WaitingForDependencies
        )
    }
}

impl GraphExecutionStore {
    /// Whether `operation_id` is a start the scheduler is still holding —
    /// a member not yet released in a live context.
    pub fn is_operation_held(&self, operation_id: OperationId) -> bool {
        self.associated_contexts(operation_id)
            .into_iter()
            .filter_map(|context_id| self.contexts.get(&context_id))
            .filter_map(|context| context.member_for_operation(operation_id))
            .any(|member| member.status.is_held())
    }
}
