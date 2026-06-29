mod checks;
mod hooks;
mod readiness;

use std::collections::{BTreeMap, VecDeque};

use crate::ids::OperationId;

pub use checks::{
    PendingPreStartCheck, PendingPreStartCheckStart, PreStartCheckDeadline, PrecheckedGraphStart,
    RunningPreStartCheckHelper,
};
pub use hooks::{
    PostStartHookDeadline, PostStartHookSequence, PreStartHookDeadline, PreStartHookSequence,
};
pub use readiness::ReadinessDeadline;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StartExecutionStore {
    pending_pre_start_checks: BTreeMap<OperationId, PendingPreStartCheck>,
    pending_pre_start_check_launches: VecDeque<OperationId>,
    running_pre_start_check_helpers: BTreeMap<i32, RunningPreStartCheckHelper>,
    pre_start_check_deadlines: BTreeMap<OperationId, PreStartCheckDeadline>,
    prechecked_graph_starts: BTreeMap<OperationId, PrecheckedGraphStart>,
    pre_start_sequences: BTreeMap<OperationId, PreStartHookSequence>,
    pre_start_hook_deadlines: BTreeMap<OperationId, PreStartHookDeadline>,
    post_start_sequences: BTreeMap<OperationId, PostStartHookSequence>,
    post_start_hook_deadlines: BTreeMap<OperationId, PostStartHookDeadline>,
    readiness_deadlines: BTreeMap<OperationId, ReadinessDeadline>,
}

impl StartExecutionStore {
    pub fn new() -> Self {
        Self::default()
    }
}
