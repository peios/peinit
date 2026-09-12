use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::fd_store::FdStoreTable;
use crate::ids::{JobId, OperationId};
use crate::job::JobStore;
use crate::logging::RuntimeLogConfig;
use crate::operation::store::OperationStore;
use crate::service::{ServiceEnvironmentVariable, ServiceTable};
use crate::shutdown::ShutdownRuntime;
use crate::supervisor::control_boundary::PendingControlOperation;

use super::{Supervisor, SupervisorSettings};

impl Supervisor {
    pub fn settings(&self) -> &SupervisorSettings {
        &self.settings
    }

    pub fn services(&self) -> &ServiceTable {
        &self.services
    }

    pub fn operations(&self) -> &OperationStore {
        &self.operations
    }

    pub fn jobs(&self) -> &JobStore {
        &self.jobs
    }

    pub fn graph(&self) -> &crate::execution::graph::GraphExecutionStore {
        &self.graph
    }

    pub fn fd_store(&self) -> &FdStoreTable {
        &self.fd_store
    }

    pub fn global_environment(&self) -> &[ServiceEnvironmentVariable] {
        &self.global_environment
    }

    pub fn control_security(&self) -> &ControlSecurityDescriptor {
        &self.control_security
    }

    pub fn control_limits(&self) -> ControlSocketLimits {
        self.control_limits
    }

    pub fn jobs_limits(&self) -> crate::jobs::socket::JobsSocketLimits {
        self.jobs_limits
    }

    pub fn submitted_jobs(&self) -> &crate::submitted::SubmittedJobStore {
        &self.submitted
    }

    pub fn pending_submitted_launch_jobs(&self) -> Vec<JobId> {
        self.pending_submitted_launches.iter().copied().collect()
    }

    pub fn log_config(&self) -> &RuntimeLogConfig {
        &self.log_config
    }

    pub fn eventd_log_socket_path(&self) -> Option<&str> {
        self.eventd_log_socket_path.as_deref()
    }

    pub fn pending_launch_jobs(&self) -> Vec<JobId> {
        self.pending_launches.iter().copied().collect()
    }

    pub fn pending_start_hook_launch_jobs(&self) -> Vec<JobId> {
        self.pending_start_hook_launches.iter().copied().collect()
    }

    pub fn pending_post_hook_launch_jobs(&self) -> Vec<JobId> {
        self.pending_post_hook_launches.iter().copied().collect()
    }

    pub fn pending_pre_start_check_launches(&self) -> Vec<OperationId> {
        self.start.pending_pre_start_check_launches()
    }

    pub fn pending_control_launch_jobs(&self) -> Vec<JobId> {
        self.pending_control_launches.iter().copied().collect()
    }

    pub fn pending_health_check_launch_jobs(&self) -> Vec<JobId> {
        self.pending_health_launches.iter().copied().collect()
    }

    pub fn pending_process_setup_fds(&self) -> Vec<i32> {
        self.pending_process_setups.keys().copied().collect()
    }

    /// Queued launch ids dropped since the last call because their job record
    /// had gone. Reset on read; the work pump reports it for the turn.
    pub fn take_stale_launch_entries(&mut self) -> usize {
        std::mem::take(&mut self.stale_launch_entries)
    }

    /// Control operations that failed before they began since the last call.
    /// Reset on read; the work pump reports them for the turn.
    pub fn take_control_operation_failures(
        &mut self,
    ) -> Vec<super::super::SupervisorControlFailureDispatch> {
        std::mem::take(&mut self.control_operation_failures)
    }

    /// Due restarts peinit could not execute since the last call. Reset on
    /// read; the lifecycle deadline timer reports them for the turn.
    pub fn take_restart_backoff_failures(
        &mut self,
    ) -> Vec<super::super::SupervisorRestartBackoffFailureDispatch> {
        std::mem::take(&mut self.restart_backoff_failures)
    }

    /// Mutate the job store directly, to stage bookkeeping faults a correct
    /// caller would not create.
    #[cfg(test)]
    pub(crate) fn jobs_mut(&mut self) -> &mut crate::job::JobStore {
        &mut self.jobs
    }

    pub fn pending_control_operations(&self) -> Vec<PendingControlOperation> {
        self.pending_control_operations.iter().cloned().collect()
    }

    #[cfg_attr(
        not(all(feature = "peios-boundary", feature = "peios-registry")),
        allow(dead_code)
    )]
    pub(crate) fn retain_service_launch_for_runtime(&mut self, launch: LaunchCreatedJobDispatch) {
        self.retained_service_launches.push(launch);
    }

    #[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
    pub(crate) fn retained_service_launches(&self) -> &[LaunchCreatedJobDispatch] {
        &self.retained_service_launches
    }

    #[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
    pub(crate) fn drain_retained_service_launches(&mut self) -> Vec<LaunchCreatedJobDispatch> {
        std::mem::take(&mut self.retained_service_launches)
    }

    pub fn shutdown(&self) -> Option<&ShutdownRuntime> {
        self.shutdown.as_ref()
    }
}
