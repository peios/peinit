mod critical;
mod fail_job;
mod failure;
mod helpers;
mod terminal;

pub(in crate::supervisor) use critical::health_critical_reboot_due;
pub(in crate::supervisor) use fail_job::{
    fail_created_health_check_in_work, fail_timed_out_health_check_in_work,
};
pub(in crate::supervisor) use terminal::apply_health_check_terminal_in_work;
