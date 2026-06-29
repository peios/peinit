use crate::ids::{JobId, OperationId};
use crate::job::JobState;
use crate::operation::store::DEFAULT_TERMINAL_OPERATION_RETENTION_NS;
use crate::operation::{OperationRecord, OperationState, OperationType};
use crate::service::ServiceDefinition;
use crate::service::runtime::ServiceState;

use super::model::{DueOperationMaintenance, RunningServiceMainStartTimeout};
use crate::supervisor::Supervisor;

const NANOS_PER_SEC: u64 = 1_000_000_000;

impl Supervisor {
    pub fn next_operation_maintenance_deadline_ns(&self) -> Option<u64> {
        [
            self.next_pending_operation_timeout_deadline_ns(),
            self.next_running_service_main_start_timeout_deadline_ns(),
            self.services.next_restart_window_reset_deadline_ns(),
            self.operations
                .next_terminal_retention_deadline_ns(DEFAULT_TERMINAL_OPERATION_RETENTION_NS),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub(in crate::supervisor) fn operation_timeout_expired(
        &self,
        operation_id: OperationId,
        now_ns: u64,
    ) -> bool {
        self.operations
            .get(operation_id)
            .and_then(|operation| self.operation_timeout_deadline_ns(operation))
            .is_some_and(|deadline_ns| deadline_ns <= now_ns)
    }

    pub(in crate::supervisor::operation_maintenance) fn due_operation_maintenance(
        &self,
        include_running_service_main_start_timeouts: bool,
        now_ns: u64,
    ) -> DueOperationMaintenance {
        DueOperationMaintenance {
            pending_operation_timeouts: self.due_pending_operation_timeouts(now_ns),
            running_service_main_start_timeouts: if include_running_service_main_start_timeouts {
                self.due_running_service_main_start_timeouts(now_ns)
            } else {
                Vec::new()
            },
            restart_window_resets: self.services.due_restart_window_resets(now_ns),
        }
    }

    pub(in crate::supervisor::operation_maintenance) fn purge_retained_terminal_operations(
        &mut self,
        now_ns: u64,
    ) -> Vec<OperationId> {
        self.operations
            .purge_terminal_retained_until(now_ns, DEFAULT_TERMINAL_OPERATION_RETENTION_NS)
    }

    fn next_pending_operation_timeout_deadline_ns(&self) -> Option<u64> {
        self.operations
            .active_records()
            .into_iter()
            .filter(|operation| operation.state == OperationState::Pending)
            .filter_map(|operation| self.operation_timeout_deadline_ns(operation))
            .min()
    }

    fn next_running_service_main_start_timeout_deadline_ns(&self) -> Option<u64> {
        self.operations
            .active_records()
            .into_iter()
            .filter(|operation| self.running_service_main_start_timeout_due_for(operation))
            .filter_map(|operation| self.operation_timeout_deadline_ns(operation))
            .min()
    }

    fn due_pending_operation_timeouts(&self, now_ns: u64) -> Vec<OperationRecord> {
        self.operations
            .active_records()
            .into_iter()
            .filter(|operation| operation.state == OperationState::Pending)
            .filter(|operation| {
                self.operation_timeout_deadline_ns(operation)
                    .is_some_and(|deadline_ns| deadline_ns <= now_ns)
            })
            .cloned()
            .collect()
    }

    fn due_running_service_main_start_timeouts(
        &self,
        now_ns: u64,
    ) -> Vec<RunningServiceMainStartTimeout> {
        self.operations
            .active_records()
            .into_iter()
            .filter(|operation| self.running_service_main_start_timeout_due_for(operation))
            .filter(|operation| {
                self.operation_timeout_deadline_ns(operation)
                    .is_some_and(|deadline_ns| deadline_ns <= now_ns)
            })
            .filter_map(|operation| {
                let job_id = self.jobs.current_service_main_job(&operation.service)?;
                Some(RunningServiceMainStartTimeout {
                    operation_id: operation.id,
                    job_id,
                })
            })
            .collect()
    }

    fn running_service_main_start_timeout_due_for(&self, operation: &OperationRecord) -> bool {
        if operation.state != OperationState::Running
            || !matches!(
                operation.operation_type,
                OperationType::Start | OperationType::Restart
            )
        {
            return false;
        }
        if self
            .services
            .runtime(&operation.service)
            .is_none_or(|runtime| runtime.state != ServiceState::Starting)
        {
            return false;
        }
        if self
            .start
            .readiness_deadline_for_service(&operation.service)
            .is_some()
        {
            return false;
        }
        let Some(job_id) = self.jobs.current_service_main_job(&operation.service) else {
            return false;
        };
        self.job_is_current_service_main_start_timeout_candidate(job_id, operation)
    }

    fn job_is_current_service_main_start_timeout_candidate(
        &self,
        job_id: JobId,
        operation: &OperationRecord,
    ) -> bool {
        self.jobs.get(job_id).is_some_and(|job| {
            job.operation_id == Some(operation.id)
                && matches!(job.state, JobState::Created | JobState::Running)
        })
    }

    fn operation_timeout_deadline_ns(&self, operation: &OperationRecord) -> Option<u64> {
        let definition = self.services.definition(&operation.service)?;
        Some(
            operation
                .created_at_ns
                .saturating_add(operation_lifetime_ns(operation.operation_type, definition)),
        )
    }
}

fn operation_lifetime_ns(operation_type: OperationType, definition: &ServiceDefinition) -> u64 {
    let seconds = match operation_type {
        OperationType::Start | OperationType::Reload | OperationType::Reset => {
            definition.start_timeout_secs
        }
        OperationType::Stop => definition.stop_timeout_secs,
        OperationType::Restart => definition
            .stop_timeout_secs
            .saturating_add(definition.start_timeout_secs),
    };
    seconds.saturating_mul(NANOS_PER_SEC)
}
