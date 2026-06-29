mod escalation;
mod interval;
mod launch;
mod model;
mod terminal;
mod terminal_apply;
mod timeout;

pub(in crate::supervisor) use escalation::terminate_service_after_health_escalation;
pub(in crate::supervisor) use interval::{
    apply_health_scheduling_after_post_start, apply_health_scheduling_after_transitions,
};
pub(in crate::supervisor) use launch::apply_started_health_check_launch;
pub(super) use model::{
    HealthCheckError, HealthCheckIntervalDeadline, HealthCheckStore, HealthCheckTimeoutDeadline,
};
pub(in crate::supervisor) use terminal_apply::{
    apply_health_check_terminal_in_work, fail_created_health_check_in_work,
    fail_timed_out_health_check_in_work, health_critical_reboot_due,
};
