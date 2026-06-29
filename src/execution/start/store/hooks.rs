use crate::ids::{JobId, OperationId};
use crate::security::TokenSummary;
use crate::service::ServiceDefinition;

use super::StartExecutionStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreStartHookSequence {
    pub service: String,
    pub operation_id: OperationId,
    pub main_job_id: JobId,
    pub definition: ServiceDefinition,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub commands: Vec<Vec<String>>,
    pub next_index: usize,
    pub deadline_ns: u64,
}

impl PreStartHookSequence {
    pub fn next_command(&self) -> Option<Vec<String>> {
        self.commands.get(self.next_index).cloned()
    }

    pub fn advance(&mut self) {
        self.next_index += 1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreStartHookDeadline {
    pub operation_id: OperationId,
    pub job_id: JobId,
    pub service: String,
    pub hooks_cgroup_id: String,
    pub service_cgroup_id: String,
    pub due_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostStartHookSequence {
    pub service: String,
    pub operation_id: OperationId,
    pub definition: ServiceDefinition,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub commands: Vec<Vec<String>>,
    pub next_index: usize,
    pub deadline_ns: u64,
    pub readiness_result: String,
    pub had_failure: bool,
}

impl PostStartHookSequence {
    pub fn next_command(&self) -> Option<Vec<String>> {
        self.commands.get(self.next_index).cloned()
    }

    pub fn advance(&mut self) {
        self.next_index += 1;
    }

    pub fn record_failure(&mut self) {
        self.had_failure = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostStartHookDeadline {
    pub operation_id: OperationId,
    pub job_id: JobId,
    pub service: String,
    pub hooks_cgroup_id: String,
    pub service_cgroup_id: String,
    pub due_at_ns: u64,
}

impl StartExecutionStore {
    pub fn record_pre_start_sequence(&mut self, sequence: PreStartHookSequence) {
        self.pre_start_sequences
            .insert(sequence.operation_id, sequence);
    }

    pub fn pre_start_sequence(&self, operation_id: OperationId) -> Option<&PreStartHookSequence> {
        self.pre_start_sequences.get(&operation_id)
    }

    pub fn pre_start_sequence_mut(
        &mut self,
        operation_id: OperationId,
    ) -> Option<&mut PreStartHookSequence> {
        self.pre_start_sequences.get_mut(&operation_id)
    }

    pub fn remove_pre_start_sequence(
        &mut self,
        operation_id: OperationId,
    ) -> Option<PreStartHookSequence> {
        self.pre_start_sequences.remove(&operation_id)
    }

    pub fn record_pre_start_hook_deadline(&mut self, deadline: PreStartHookDeadline) {
        self.pre_start_hook_deadlines
            .insert(deadline.operation_id, deadline);
    }

    pub fn remove_pre_start_hook_deadline(
        &mut self,
        operation_id: OperationId,
    ) -> Option<PreStartHookDeadline> {
        self.pre_start_hook_deadlines.remove(&operation_id)
    }

    pub fn due_pre_start_hook_deadlines(&self, now_ns: u64) -> Vec<PreStartHookDeadline> {
        self.pre_start_hook_deadlines
            .values()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .cloned()
            .collect()
    }

    pub fn next_pre_start_hook_timeout(&self) -> Option<PreStartHookDeadline> {
        self.pre_start_hook_deadlines
            .values()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
            .cloned()
    }

    pub fn record_post_start_sequence(&mut self, sequence: PostStartHookSequence) {
        self.post_start_sequences
            .insert(sequence.operation_id, sequence);
    }

    pub fn post_start_sequence(&self, operation_id: OperationId) -> Option<&PostStartHookSequence> {
        self.post_start_sequences.get(&operation_id)
    }

    pub fn remove_post_start_sequence(
        &mut self,
        operation_id: OperationId,
    ) -> Option<PostStartHookSequence> {
        self.post_start_sequences.remove(&operation_id)
    }

    pub fn record_post_start_hook_deadline(&mut self, deadline: PostStartHookDeadline) {
        self.post_start_hook_deadlines
            .insert(deadline.operation_id, deadline);
    }

    pub fn remove_post_start_hook_deadline(
        &mut self,
        operation_id: OperationId,
    ) -> Option<PostStartHookDeadline> {
        self.post_start_hook_deadlines.remove(&operation_id)
    }

    pub fn due_post_start_hook_deadlines(&self, now_ns: u64) -> Vec<PostStartHookDeadline> {
        self.post_start_hook_deadlines
            .values()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .cloned()
            .collect()
    }

    pub fn next_post_start_hook_timeout(&self) -> Option<PostStartHookDeadline> {
        self.post_start_hook_deadlines
            .values()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
            .cloned()
    }
}
