mod classify;
mod pre_start_hook;
mod service;

pub(in crate::supervisor) use pre_start_hook::apply_pre_start_hook_launch_failure;
pub(in crate::supervisor) use service::apply_service_launch_failure;
