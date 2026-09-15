use crate::boundary::{BoundaryError, KmesEvent};
use crate::execution::control::ControlExecutionDetail;
use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::supervisor::{
    SupervisorControlDispatch, SupervisorHealthCheckLaunchCancelledDispatch,
    SupervisorHealthCheckLaunchDispatch, SupervisorHealthCheckLaunchFailureDispatch,
    SupervisorLaunchDispatch, SupervisorLaunchFailureDispatch,
    SupervisorPostStartHookLaunchDispatch, SupervisorPostStartHookLaunchFailureDispatch,
    SupervisorStartHookLaunchDispatch, SupervisorStartHookLaunchFailureDispatch,
};

use super::super::event::{push_job, push_operation};
use super::start::{
    collect_post_start_hook_terminal, collect_service_main_started, collect_start_dispatches,
    collect_start_failure,
};
use super::terminal::collect_health_check_terminal;

pub(in crate::runtime::kmes) fn collect_control_dispatch(
    dispatch: &SupervisorControlDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    let execution = &dispatch.execution;
    push_operation(out, &execution.operation_event)?;
    if let ControlExecutionDetail::ReloadCommand { job_event, .. } = &execution.detail {
        push_job(out, job_event)?;
    }
    for job_event in &execution.cancelled_reload_jobs {
        push_job(out, job_event)?;
    }
    Ok(())
}

pub(in crate::runtime::kmes) fn collect_service_launch(
    dispatch: &SupervisorLaunchDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_launch(&dispatch.launch, out)?;
    collect_service_main_started(&dispatch.started, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

pub(in crate::runtime::kmes) fn collect_service_launch_failure(
    dispatch: &SupervisorLaunchFailureDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    collect_start_failure(&dispatch.failure, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

pub(in crate::runtime::kmes) fn collect_start_hook_launch(
    dispatch: &SupervisorStartHookLaunchDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_launch(&dispatch.launch, out)
}

pub(in crate::runtime::kmes) fn collect_start_hook_launch_failure(
    dispatch: &SupervisorStartHookLaunchFailureDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    collect_start_failure(&dispatch.failure, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

pub(in crate::runtime::kmes) fn collect_post_start_hook_launch(
    dispatch: &SupervisorPostStartHookLaunchDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_launch(&dispatch.launch, out)
}

pub(in crate::runtime::kmes) fn collect_post_start_hook_launch_failure(
    dispatch: &SupervisorPostStartHookLaunchFailureDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_post_start_hook_terminal(&dispatch.terminal, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

pub(in crate::runtime::kmes) fn collect_health_check_launch(
    dispatch: &SupervisorHealthCheckLaunchDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_launch(&dispatch.launch, out)
}

pub(in crate::runtime::kmes) fn collect_health_check_launch_failure(
    dispatch: &SupervisorHealthCheckLaunchFailureDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_health_check_terminal(&dispatch.terminal, out)
}

pub(in crate::runtime::kmes) fn collect_health_check_launch_cancellation(
    dispatch: &SupervisorHealthCheckLaunchCancelledDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)
}

pub(in crate::runtime::kmes) fn collect_launch(
    dispatch: &LaunchCreatedJobDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)
}
