use crate::boundary::{BoundaryError, KmesEvent};
use crate::execution::control::ReloadCommandTimeoutDispatch;
use crate::execution::restart_policy::RestartPolicyRelaunchDispatch;
use crate::supervisor::{
    SupervisorHealthCheckIntervalAction, SupervisorHealthCheckIntervalDispatch,
    SupervisorRestartBackoffDispatch, SupervisorWatchdogTimeoutDispatch,
};

use super::super::event::{
    collect_on_demand_start, push_critical_failure, push_job, push_operation,
};
use super::start::collect_start_dispatches;

pub(in crate::runtime::kmes) fn collect_reload_command_timeout(
    dispatch: &ReloadCommandTimeoutDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    push_operation(out, &dispatch.operation_event)
}

pub(in crate::runtime::kmes) fn collect_restart_backoff(
    dispatch: &SupervisorRestartBackoffDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_restart_relaunch(&dispatch.relaunch, out)
}

pub(in crate::runtime::kmes) fn collect_health_check_interval(
    dispatch: &SupervisorHealthCheckIntervalDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    if let SupervisorHealthCheckIntervalAction::Created { job_event } = &dispatch.action {
        push_job(out, job_event)?;
    }
    Ok(())
}

pub(in crate::runtime::kmes) fn collect_watchdog_timeout(
    dispatch: &SupervisorWatchdogTimeoutDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    if let Some(job_event) = &dispatch.job_event {
        push_job(out, job_event)?;
    }
    if let Some(finalization) = &dispatch.critical_reboot {
        push_critical_failure(
            out,
            &dispatch.service,
            "watchdog_timeout",
            Some(dispatch.timed_out_at_ns),
            finalization,
        )?;
    }
    Ok(())
}

fn collect_restart_relaunch(
    dispatch: &RestartPolicyRelaunchDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_on_demand_start(&dispatch.admission, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}
