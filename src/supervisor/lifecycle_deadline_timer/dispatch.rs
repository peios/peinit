use crate::supervisor::SupervisorCriticalBudgetRebootDispatch;
use crate::supervisor::cgroup_cleanup::SupervisorLeakedCgroupDispatch;
use crate::supervisor::dispatch::{
    SupervisorBootSettleDispatch, SupervisorBootSuccessDispatch,
    SupervisorFilesystemCheckTimeoutDispatch, SupervisorHealthCheckIntervalDispatch,
    SupervisorHealthCheckTimeoutDispatch, SupervisorPostStartHookTimeoutDispatch,
    SupervisorPreStartHookTimeoutDispatch, SupervisorReadinessTimeoutDispatch,
    SupervisorReloadCommandTimeoutDispatch, SupervisorReloadDetectionDispatch,
    SupervisorRestartBackoffDispatch, SupervisorRestartBackoffFailureDispatch,
    SupervisorStopEscalationDispatch, SupervisorSubmittedDeadlineDispatch,
    SupervisorWatchdogTimeoutDispatch,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupervisorLifecycleDeadlineDispatch {
    pub pre_start_check_timeouts: Vec<SupervisorFilesystemCheckTimeoutDispatch>,
    pub pre_start_hook_timeouts: Vec<SupervisorPreStartHookTimeoutDispatch>,
    pub post_start_hook_timeouts: Vec<SupervisorPostStartHookTimeoutDispatch>,
    pub readiness_timeouts: Vec<SupervisorReadinessTimeoutDispatch>,
    pub stop_timeouts: Vec<SupervisorStopEscalationDispatch>,
    pub reload_detections: Vec<SupervisorReloadDetectionDispatch>,
    pub reload_command_timeouts: Vec<SupervisorReloadCommandTimeoutDispatch>,
    pub restart_backoffs: Vec<SupervisorRestartBackoffDispatch>,
    /// Due restarts peinit could not execute (PEI-808).
    pub restart_backoff_failures: Vec<SupervisorRestartBackoffFailureDispatch>,
    pub health_check_intervals: Vec<SupervisorHealthCheckIntervalDispatch>,
    pub health_check_timeouts: Vec<SupervisorHealthCheckTimeoutDispatch>,
    pub watchdog_timeouts: Vec<SupervisorWatchdogTimeoutDispatch>,
    pub boot_successes: Vec<SupervisorBootSuccessDispatch>,
    pub boot_settles: Vec<SupervisorBootSettleDispatch>,
    pub cgroup_leaks: Vec<SupervisorLeakedCgroupDispatch>,
    pub submitted_jobs: Vec<SupervisorSubmittedDeadlineDispatch>,
    /// The immediate reboot owed to a Critical service that exhausted its
    /// restart budget during this turn by a route with no reboot check of its
    /// own — a readiness timeout, a pre-start hook or check timeout.
    ///
    /// On the dispatch rather than on one of the lists above because the
    /// reboot is about the service, not about which deadline happened to
    /// notice it, and enumerating the deadlines that can cause one is what
    /// went wrong the first time (PEI-341).
    pub critical_budget_reboot: Option<SupervisorCriticalBudgetRebootDispatch>,
}

impl SupervisorLifecycleDeadlineDispatch {
    pub fn is_empty(&self) -> bool {
        self.pre_start_check_timeouts.is_empty()
            && self.pre_start_hook_timeouts.is_empty()
            && self.post_start_hook_timeouts.is_empty()
            && self.readiness_timeouts.is_empty()
            && self.stop_timeouts.is_empty()
            && self.reload_detections.is_empty()
            && self.reload_command_timeouts.is_empty()
            && self.restart_backoffs.is_empty()
            && self.restart_backoff_failures.is_empty()
            && self.health_check_intervals.is_empty()
            && self.health_check_timeouts.is_empty()
            && self.watchdog_timeouts.is_empty()
            && self.boot_successes.is_empty()
            && self.boot_settles.is_empty()
            && self.cgroup_leaks.is_empty()
            && self.submitted_jobs.is_empty()
            && self.critical_budget_reboot.is_none()
    }
}
