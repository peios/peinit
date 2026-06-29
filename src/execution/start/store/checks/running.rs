use crate::boundary::LaunchedFilesystemCheckHelper;
use crate::ids::OperationId;

use super::super::StartExecutionStore;
use super::model::{PreStartCheckDeadline, RunningPreStartCheckHelper};

impl StartExecutionStore {
    pub fn record_running_pre_start_check_helper(
        &mut self,
        helper: LaunchedFilesystemCheckHelper,
        launched_at_ns: u64,
    ) -> Option<RunningPreStartCheckHelper> {
        let pending = self
            .pending_pre_start_checks
            .get(&helper.operation_id)?
            .clone();
        let running = RunningPreStartCheckHelper { helper, pending };
        self.pre_start_check_deadlines.insert(
            running.pending.operation_id,
            PreStartCheckDeadline {
                operation_id: running.pending.operation_id,
                service: running.pending.service.clone(),
                helper_cgroup_id: running.pending.helper_cgroup_id(),
                result_fd: running.helper.result_fd,
                pidfd: running.helper.pidfd,
                due_at_ns: deadline_ns(launched_at_ns, running.pending.timeout_secs)
                    .min(running.pending.operation_deadline_ns),
            },
        );
        self.running_pre_start_check_helpers
            .insert(running.helper.result_fd, running.clone());
        Some(running)
    }

    pub fn running_pre_start_check_helper_by_result_fd(
        &self,
        result_fd: i32,
    ) -> Option<&RunningPreStartCheckHelper> {
        self.running_pre_start_check_helpers.get(&result_fd)
    }

    pub fn running_pre_start_check_helper_by_pidfd(
        &self,
        pidfd: i32,
    ) -> Option<&RunningPreStartCheckHelper> {
        self.running_pre_start_check_helpers
            .values()
            .find(|running| running.helper.pidfd == pidfd)
    }

    pub fn remove_running_pre_start_check_helper_by_result_fd(
        &mut self,
        result_fd: i32,
    ) -> Option<RunningPreStartCheckHelper> {
        let running = self.running_pre_start_check_helpers.remove(&result_fd)?;
        self.pre_start_check_deadlines
            .remove(&running.pending.operation_id);
        self.pending_pre_start_checks
            .remove(&running.pending.operation_id);
        Some(running)
    }

    pub fn remove_running_pre_start_check_helper_by_operation_id(
        &mut self,
        operation_id: OperationId,
    ) -> Option<RunningPreStartCheckHelper> {
        let result_fd =
            self.running_pre_start_check_helpers
                .iter()
                .find_map(|(result_fd, running)| {
                    (running.pending.operation_id == operation_id).then_some(*result_fd)
                })?;
        self.remove_running_pre_start_check_helper_by_result_fd(result_fd)
    }
}

const NANOS_PER_SEC: u64 = 1_000_000_000;

fn deadline_ns(started_at_ns: u64, timeout_secs: u64) -> u64 {
    started_at_ns.saturating_add(timeout_secs.saturating_mul(NANOS_PER_SEC))
}
