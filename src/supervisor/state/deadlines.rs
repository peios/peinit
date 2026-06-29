use crate::execution::control::{
    ReloadCommandDeadline, ReloadDetectionDeadline, StopTimeoutDeadline,
};
use crate::execution::start::{
    PostStartHookDeadline, PreStartCheckDeadline, PreStartHookDeadline, ReadinessDeadline,
};
use crate::service::RestartBackoffDeadline;
use crate::supervisor::health::{HealthCheckIntervalDeadline, HealthCheckTimeoutDeadline};
use crate::supervisor::watchdog::WatchdogDeadline;

use super::Supervisor;

impl Supervisor {
    pub fn next_restart_backoff_deadline(&self) -> Option<RestartBackoffDeadline> {
        self.services.next_restart_backoff_deadline()
    }

    pub fn next_stop_timeout_deadline(&self) -> Option<StopTimeoutDeadline> {
        self.control.next_stop_timeout()
    }

    pub fn next_pre_start_hook_timeout(&self) -> Option<PreStartHookDeadline> {
        self.start.next_pre_start_hook_timeout()
    }

    pub fn next_post_start_hook_timeout(&self) -> Option<PostStartHookDeadline> {
        self.start.next_post_start_hook_timeout()
    }

    pub fn next_pre_start_check_timeout(&self) -> Option<PreStartCheckDeadline> {
        self.start.next_pre_start_check_timeout()
    }

    pub fn next_readiness_timeout(&self) -> Option<ReadinessDeadline> {
        self.start.next_readiness_timeout()
    }

    pub fn next_health_check_interval(&self) -> Option<HealthCheckIntervalDeadline> {
        self.health.next_interval_deadline()
    }

    pub fn next_health_check_timeout(&self) -> Option<HealthCheckTimeoutDeadline> {
        self.health.next_timeout_deadline()
    }

    pub fn next_watchdog_timeout(&self) -> Option<WatchdogDeadline> {
        self.watchdog.next_deadline()
    }

    pub fn due_reload_detection_deadlines(&self, now_ns: u64) -> Vec<ReloadDetectionDeadline> {
        self.control.due_reload_detection_deadlines(now_ns)
    }

    pub fn next_reload_detection_deadline(&self) -> Option<ReloadDetectionDeadline> {
        self.control.next_reload_detection_deadline()
    }

    pub fn next_reload_command_timeout(&self) -> Option<ReloadCommandDeadline> {
        self.control.next_reload_command_deadline()
    }
}
