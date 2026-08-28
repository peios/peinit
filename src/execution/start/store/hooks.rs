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

    /// Forget every post-start hook sequence and deadline a service has, and
    /// hand back the deadlines so the caller can kill the hooks cgroups they
    /// name.
    ///
    /// For a service being stopped while its `ExecStartPost` hooks are still
    /// running. Nothing else removes that state: the hooks' cgroup goes with
    /// the service's tree, but the deadline stayed armed and fired later into a
    /// cgroup that no longer existed — an error on the lifecycle-deadline path,
    /// which put PID 1 into recovery (PEI-491). A stopped service has no
    /// post-start sequence to time out.
    pub fn cancel_post_start_for_service(&mut self, service: &str) -> Vec<PostStartHookDeadline> {
        self.post_start_sequences
            .retain(|_, sequence| sequence.service != service);
        let (cancelled, kept): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.post_start_hook_deadlines)
                .into_iter()
                .partition(|(_, deadline)| deadline.service == service);
        self.post_start_hook_deadlines = kept.into_iter().collect();
        cancelled
            .into_iter()
            .map(|(_, deadline)| deadline)
            .collect()
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

#[cfg(test)]
mod tests {
    use super::{PostStartHookDeadline, PostStartHookSequence, StartExecutionStore};
    use crate::ids::{JobId, JobIdAllocator, OperationId, OperationIdAllocator};
    use crate::security::TokenSummary;
    use crate::service::ServiceDefinition;

    fn sequence(service: &str, operation_id: OperationId) -> PostStartHookSequence {
        PostStartHookSequence {
            service: service.to_string(),
            operation_id,
            definition: ServiceDefinition::simple_system_boot(service, "/sbin/x"),
            resolved_identity: "SYSTEM".to_string(),
            token_summary: TokenSummary::new("SYSTEM", "S-1-5-18", vec![], vec![], vec![]),
            activation_generation: 1,
            cgroup_generation: 1,
            commands: vec![vec!["/libexec/hook".to_string()]],
            next_index: 1,
            deadline_ns: 60,
            readiness_result: "alive".to_string(),
            had_failure: false,
        }
    }

    fn deadline(service: &str, operation_id: OperationId, job_id: JobId) -> PostStartHookDeadline {
        PostStartHookDeadline {
            operation_id,
            job_id,
            service: service.to_string(),
            hooks_cgroup_id: format!("/sys/fs/cgroup/peinit/{service}/hooks"),
            service_cgroup_id: format!("/sys/fs/cgroup/peinit/{service}"),
            due_at_ns: 60,
        }
    }

    /// PEI-491: a stop must forget the stopped service's post-start hooks —
    /// sequence and deadline both — and only that service's.
    #[test]
    fn cancel_post_start_for_service_forgets_only_that_service() {
        let mut store = StartExecutionStore::default();
        let ops = OperationIdAllocator::new()
            .allocate_batch(2, 1_000)
            .expect("operation ids");
        let jobs = JobIdAllocator::new()
            .allocate_batch(2, 1_000)
            .expect("job ids");
        let (a, b) = (ops[0], ops[1]);
        store.record_post_start_sequence(sequence("eudev", a));
        store.record_post_start_hook_deadline(deadline("eudev", a, jobs[0]));
        store.record_post_start_sequence(sequence("authd", b));
        store.record_post_start_hook_deadline(deadline("authd", b, jobs[1]));

        let cancelled = store.cancel_post_start_for_service("eudev");

        assert_eq!(cancelled.len(), 1);
        assert_eq!(
            cancelled[0].hooks_cgroup_id,
            "/sys/fs/cgroup/peinit/eudev/hooks"
        );
        assert!(store.post_start_sequence(a).is_none());
        assert!(
            store
                .due_post_start_hook_deadlines(u64::MAX)
                .iter()
                .all(|d| d.service == "authd")
        );
        assert!(store.post_start_sequence(b).is_some());
        assert_eq!(store.due_post_start_hook_deadlines(u64::MAX).len(), 1);
    }

    #[test]
    fn cancel_post_start_for_service_without_hooks_is_a_no_op() {
        let mut store = StartExecutionStore::default();
        assert!(store.cancel_post_start_for_service("eudev").is_empty());
    }
}
