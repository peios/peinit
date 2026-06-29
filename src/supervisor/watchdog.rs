mod model;
mod notify;
mod scheduling;
mod timeout;

pub(in crate::supervisor) use model::{
    WatchdogDeadline, WatchdogError, WatchdogStore, WatchdogUpdate,
};
pub(in crate::supervisor) use notify::apply_watchdog_notify_fields;
pub(in crate::supervisor) use scheduling::{
    apply_watchdog_scheduling_after_post_start, apply_watchdog_scheduling_after_transitions,
};
