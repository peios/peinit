use crate::boundary::{BoundaryError, KmesEvent};
use crate::execution::control::ReloadDetectionCompletion;
use crate::supervisor::{
    SupervisorLifecycleDeadlineDispatch, SupervisorReadinessTimeoutDispatch,
    SupervisorReloadCommandTimeoutDispatch, SupervisorReloadDetectionDispatch,
};

use super::super::event::push_operation;
use super::super::job::{
    collect_health_check_interval, collect_health_check_terminal, collect_post_start_hook_timeout,
    collect_pre_start_check_timeout, collect_pre_start_hook_timeout, collect_readiness_timeout,
    collect_reload_command_timeout, collect_restart_backoff, collect_start_dispatches,
    collect_watchdog_timeout,
};

pub(in crate::runtime::kmes) fn collect_lifecycle_deadline_dispatch(
    dispatch: &SupervisorLifecycleDeadlineDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    for timeout in &dispatch.pre_start_check_timeouts {
        collect_pre_start_check_timeout(&timeout.timeout, out)?;
        collect_start_dispatches(&timeout.start_dispatches, out)?;
    }
    for timeout in &dispatch.pre_start_hook_timeouts {
        collect_pre_start_hook_timeout(&timeout.timeout, out)?;
        collect_start_dispatches(&timeout.start_dispatches, out)?;
    }
    for timeout in &dispatch.post_start_hook_timeouts {
        collect_post_start_hook_timeout(&timeout.timeout, out)?;
        collect_start_dispatches(&timeout.start_dispatches, out)?;
    }
    for timeout in &dispatch.readiness_timeouts {
        collect_readiness_timeout_dispatch(timeout, out)?;
    }
    for detection in &dispatch.reload_detections {
        collect_reload_detection(detection, out)?;
    }
    for timeout in &dispatch.reload_command_timeouts {
        collect_reload_command_timeout_dispatch(timeout, out)?;
    }
    for backoff in &dispatch.restart_backoffs {
        collect_restart_backoff(backoff, out)?;
    }
    for failure in &dispatch.restart_backoff_failures {
        if let Some(event) = &failure.operation_event {
            super::super::event::push_operation(out, event)?;
        }
    }
    for interval in &dispatch.health_check_intervals {
        collect_health_check_interval(interval, out)?;
    }
    for timeout in &dispatch.health_check_timeouts {
        collect_health_check_terminal(&timeout.terminal, out)?;
    }
    for timeout in &dispatch.watchdog_timeouts {
        collect_watchdog_timeout(timeout, out)?;
    }
    for leak in &dispatch.cgroup_leaks {
        out.push(crate::kmes::encode_leaked_cgroup_event(leak)?);
    }
    for dispatch in &dispatch.submitted_jobs {
        super::super::submitted::collect_submitted_deadline(dispatch, out)?;
    }
    for failure in &dispatch.internal_errors {
        super::super::event::push_internal_error(out, failure)?;
    }
    Ok(())
}

fn collect_readiness_timeout_dispatch(
    dispatch: &SupervisorReadinessTimeoutDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_readiness_timeout(&dispatch.timeout, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

fn collect_reload_detection(
    dispatch: &SupervisorReloadDetectionDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    let ReloadDetectionCompletion {
        operation_event,
        phase,
        ..
    } = &dispatch.completion;
    // A service that announced RELOADING=1 and never finished has either
    // wedged or lost its handler. The operation event alone carries no
    // severity and, since `reload` defaults to wait=false, nobody is
    // necessarily reading it (PEI-359).
    if *phase == crate::execution::control::ReloadDetectionPhase::ExtendedWait {
        out.push(crate::kmes::encode_reload_unconfirmed_event(
            &operation_event.service,
        )?);
    }
    push_operation(out, operation_event)
}

fn collect_reload_command_timeout_dispatch(
    dispatch: &SupervisorReloadCommandTimeoutDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_reload_command_timeout(&dispatch.timeout, out)
}
