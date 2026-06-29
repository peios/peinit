use crate::boundary::{BoundaryError, KmesEvent};
use crate::execution::control::ReloadCommandTerminalDispatch;
use crate::execution::job_terminal::ServiceMainJobTerminalDispatch;
use crate::execution::start::RestartStartExecutionDispatch;
use crate::supervisor::{SupervisorHealthCheckTerminalDispatch, SupervisorTerminalDispatch};

use super::super::event::{
    push_critical_failure, push_graphs, push_job, push_operation, push_operations,
};
use super::start::collect_start_dispatches;

pub(in crate::runtime::kmes) fn collect_terminal_dispatch(
    dispatch: &SupervisorTerminalDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_service_main_terminal(&dispatch.terminal, out)?;
    for event in &dispatch.cleanup_job_events {
        push_job(out, event)?;
    }
    collect_start_dispatches(&dispatch.start_dispatches, out)?;
    collect_restart_start_dispatches(&dispatch.restart_start_dispatches, out)?;
    if let Some(finalization) = &dispatch.critical_reboot
        && let Some(service) = dispatch.terminal.job_event.service.as_deref()
    {
        push_critical_failure(
            out,
            service,
            "service_main_terminal",
            dispatch.terminal.job_event.ended_at_ns,
            finalization,
        )?;
    }
    Ok(())
}

pub(in crate::runtime::kmes) fn collect_health_check_terminal(
    dispatch: &SupervisorHealthCheckTerminalDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    if let Some(job_event) = &dispatch.service_job_event {
        push_job(out, job_event)?;
    }
    if let Some(finalization) = &dispatch.critical_reboot
        && let Some(service) = dispatch.job_event.service.as_deref()
    {
        push_critical_failure(
            out,
            service,
            "health_check_failure",
            dispatch.job_event.ended_at_ns,
            finalization,
        )?;
    }
    Ok(())
}

pub(in crate::runtime::kmes) fn collect_reload_command_terminal(
    dispatch: &ReloadCommandTerminalDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    push_operation(out, &dispatch.operation_event)
}

fn collect_service_main_terminal(
    dispatch: &ServiceMainJobTerminalDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)?;
    if let Some(job_event) = &dispatch.post_start_hook {
        push_job(out, job_event)?;
    }
    Ok(())
}

fn collect_restart_start_dispatches(
    dispatches: &[RestartStartExecutionDispatch],
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    for dispatch in dispatches {
        push_job(out, &dispatch.job_event)?;
    }
    Ok(())
}
