mod convert;
mod kind;

use std::cmp::Ordering;

pub use kind::SupervisorLifecycleDeadlineKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLifecycleDeadline {
    pub due_at_ns: u64,
    pub kind: SupervisorLifecycleDeadlineKind,
}

impl SupervisorLifecycleDeadline {
    pub(super) fn cmp_schedule(left: &Self, right: &Self) -> Ordering {
        left.due_at_ns
            .cmp(&right.due_at_ns)
            .then_with(|| left.kind.rank().cmp(&right.kind.rank()))
            .then_with(|| left.kind.service().cmp(right.kind.service()))
            .then_with(|| left.kind.operation_id().cmp(&right.kind.operation_id()))
            .then_with(|| left.kind.job_id().cmp(&right.kind.job_id()))
    }
}
