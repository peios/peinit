use crate::boundary::{
    Clock, FilesystemCheckHelperLauncher, ProcessController, ProcessLauncher, TokenProvider,
};
use crate::supervisor::{
    SupervisorControlDispatch, SupervisorControlLaunchDispatch, SupervisorError,
    SupervisorFilesystemCheckLaunchDispatch, SupervisorHealthCheckLaunchCancelledDispatch,
    SupervisorHealthCheckLaunchDispatch, SupervisorHealthCheckLaunchFailureDispatch,
    SupervisorLaunchDispatch, SupervisorLaunchFailureDispatch,
    SupervisorPendingProcessSetupDispatch, SupervisorPostStartHookLaunchDispatch,
    SupervisorPostStartHookLaunchFailureDispatch, SupervisorStartHookLaunchDispatch,
    SupervisorStartHookLaunchFailureDispatch, SupervisorSubmittedLaunchDispatch,
    SupervisorSubmittedLaunchFailureDispatch,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeWorkPumpConfig {
    pub max_iterations: usize,
}

impl RuntimeWorkPumpConfig {
    pub const DEFAULT_MAX_ITERATIONS: usize = 1024;
}

impl Default for RuntimeWorkPumpConfig {
    fn default() -> Self {
        Self {
            max_iterations: Self::DEFAULT_MAX_ITERATIONS,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeWorkPumpTurn {
    pub iterations: usize,
    pub control_operations: Vec<SupervisorControlDispatch>,
    pub filesystem_check_launches: Vec<SupervisorFilesystemCheckLaunchDispatch>,
    pub start_hook_launches: Vec<SupervisorStartHookLaunchDispatch>,
    pub start_hook_launch_failures: Vec<SupervisorStartHookLaunchFailureDispatch>,
    pub post_hook_launches: Vec<SupervisorPostStartHookLaunchDispatch>,
    pub post_hook_launch_failures: Vec<SupervisorPostStartHookLaunchFailureDispatch>,
    pub control_launches: Vec<SupervisorControlLaunchDispatch>,
    pub pending_process_setups: Vec<SupervisorPendingProcessSetupDispatch>,
    pub health_check_launches: Vec<SupervisorHealthCheckLaunchDispatch>,
    pub health_check_launch_failures: Vec<SupervisorHealthCheckLaunchFailureDispatch>,
    pub health_check_launch_cancellations: Vec<SupervisorHealthCheckLaunchCancelledDispatch>,
    pub service_launches: Vec<SupervisorLaunchDispatch>,
    pub service_launch_failures: Vec<SupervisorLaunchFailureDispatch>,
    pub submitted_launches: Vec<SupervisorSubmittedLaunchDispatch>,
    pub submitted_launch_failures: Vec<SupervisorSubmittedLaunchFailureDispatch>,
    pub stale_control_operations: usize,
    /// Queue entries dropped because their job record had gone (PEI-605).
    pub stale_launch_entries: usize,
}

impl RuntimeWorkPumpTurn {
    pub fn is_empty(&self) -> bool {
        self.iterations == 0
            && self.control_operations.is_empty()
            && self.filesystem_check_launches.is_empty()
            && self.start_hook_launches.is_empty()
            && self.start_hook_launch_failures.is_empty()
            && self.post_hook_launches.is_empty()
            && self.post_hook_launch_failures.is_empty()
            && self.control_launches.is_empty()
            && self.pending_process_setups.is_empty()
            && self.health_check_launches.is_empty()
            && self.health_check_launch_failures.is_empty()
            && self.health_check_launch_cancellations.is_empty()
            && self.service_launches.is_empty()
            && self.service_launch_failures.is_empty()
            && self.submitted_launches.is_empty()
            && self.submitted_launch_failures.is_empty()
            && self.stale_control_operations == 0
            && self.stale_launch_entries == 0
    }

    pub(super) fn extend(&mut self, step: RuntimeWorkPumpStep) {
        self.iterations += 1;
        self.control_operations.extend(step.control_operation);
        self.filesystem_check_launches
            .extend(step.filesystem_check_launch);
        self.start_hook_launches.extend(step.start_hook_launch);
        self.start_hook_launch_failures
            .extend(step.start_hook_launch_failure);
        self.post_hook_launches.extend(step.post_hook_launch);
        self.post_hook_launch_failures
            .extend(step.post_hook_launch_failure);
        self.control_launches.extend(step.control_launch);
        self.pending_process_setups
            .extend(step.pending_process_setups);
        self.health_check_launches.extend(step.health_check_launch);
        self.health_check_launch_failures
            .extend(step.health_check_launch_failure);
        self.health_check_launch_cancellations
            .extend(step.health_check_launch_cancellation);
        self.service_launches.extend(step.service_launch);
        self.service_launch_failures
            .extend(step.service_launch_failure);
        self.submitted_launches.extend(step.submitted_launch);
        self.submitted_launch_failures
            .extend(step.submitted_launch_failure);
        self.stale_control_operations += step.stale_control_operations;
        self.stale_launch_entries += step.stale_launch_entries;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeWorkPumpStep {
    pub control_operation: Option<SupervisorControlDispatch>,
    pub filesystem_check_launch: Option<SupervisorFilesystemCheckLaunchDispatch>,
    pub start_hook_launch: Option<SupervisorStartHookLaunchDispatch>,
    pub start_hook_launch_failure: Option<SupervisorStartHookLaunchFailureDispatch>,
    pub post_hook_launch: Option<SupervisorPostStartHookLaunchDispatch>,
    pub post_hook_launch_failure: Option<SupervisorPostStartHookLaunchFailureDispatch>,
    pub control_launch: Option<SupervisorControlLaunchDispatch>,
    pub pending_process_setups: Vec<SupervisorPendingProcessSetupDispatch>,
    pub health_check_launch: Option<SupervisorHealthCheckLaunchDispatch>,
    pub health_check_launch_failure: Option<SupervisorHealthCheckLaunchFailureDispatch>,
    pub health_check_launch_cancellation: Option<SupervisorHealthCheckLaunchCancelledDispatch>,
    pub service_launch: Option<SupervisorLaunchDispatch>,
    pub service_launch_failure: Option<SupervisorLaunchFailureDispatch>,
    pub submitted_launch: Option<SupervisorSubmittedLaunchDispatch>,
    pub submitted_launch_failure: Option<SupervisorSubmittedLaunchFailureDispatch>,
    pub stale_control_operations: usize,
    /// Queue entries dropped because their job record had gone (PEI-605).
    pub stale_launch_entries: usize,
}

impl RuntimeWorkPumpStep {
    pub(super) fn progressed(&self) -> bool {
        self.control_operation.is_some()
            || self.filesystem_check_launch.is_some()
            || self.start_hook_launch.is_some()
            || self.start_hook_launch_failure.is_some()
            || self.post_hook_launch.is_some()
            || self.post_hook_launch_failure.is_some()
            || self.control_launch.is_some()
            || !self.pending_process_setups.is_empty()
            || self.health_check_launch.is_some()
            || self.health_check_launch_failure.is_some()
            || self.health_check_launch_cancellation.is_some()
            || self.service_launch.is_some()
            || self.service_launch_failure.is_some()
            || self.submitted_launch.is_some()
            || self.submitted_launch_failure.is_some()
            || self.stale_control_operations > 0
            || self.stale_launch_entries > 0
    }
}

#[derive(Debug)]
pub enum RuntimeWorkPumpError {
    Supervisor(SupervisorError),
    IterationLimitExceeded {
        limit: usize,
        pending_control_operations: usize,
        pending_filesystem_check_launches: usize,
        pending_start_hook_launches: usize,
        pending_post_hook_launches: usize,
        pending_control_launches: usize,
        pending_health_check_launches: usize,
        pending_service_launches: usize,
        pending_submitted_launches: usize,
    },
}

pub struct RuntimeWorkPumpContext<'a, C, P, T, L, F>
where
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    T: TokenProvider + ?Sized,
    L: ProcessLauncher + ?Sized,
    F: FilesystemCheckHelperLauncher + ?Sized,
{
    pub clock: &'a mut C,
    pub controller: &'a mut P,
    pub token_provider: &'a mut T,
    pub process_launcher: &'a mut L,
    pub filesystem_check_launcher: &'a mut F,
    pub config: RuntimeWorkPumpConfig,
}
