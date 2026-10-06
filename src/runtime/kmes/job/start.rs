use crate::boundary::BoundaryError;
use crate::kmes::EventCollector;
use crate::execution::failure::StartFailureDispatch;
use crate::execution::job_started::ServiceMainJobStartedDispatch;
use crate::execution::start::{
    PostStartHookTerminalDispatch, PostStartHookTimeoutDispatch, PreStartCheckCompletionDispatch,
    PreStartCheckTimeoutDispatch, PreStartHookTerminalDispatch, PreStartHookTimeoutDispatch,
    ReadinessTimeoutDispatch, ServiceMainStartTimeoutDispatch, StartExecutionDispatch,
};

use super::super::event::{push_graphs, push_job, push_operations};

pub(in crate::runtime::kmes) fn collect_start_dispatches(
    dispatches: &[StartExecutionDispatch],
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    for dispatch in dispatches {
        push_operations(out, std::slice::from_ref(&dispatch.operation_event))?;
        push_job(out, &dispatch.job_event)?;
    }
    Ok(())
}

pub(in crate::runtime::kmes) fn collect_pre_start_check_completion(
    dispatch: &PreStartCheckCompletionDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    if let Some(job_event) = &dispatch.job_event {
        push_job(out, job_event)?;
    }
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)
}

pub(in crate::runtime::kmes) fn collect_pre_start_check_timeout(
    dispatch: &PreStartCheckTimeoutDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    collect_pre_start_check_completion(&dispatch.completion, out)
}

pub(in crate::runtime::kmes) fn collect_pre_start_hook_terminal_dispatch(
    dispatch: &crate::supervisor::SupervisorPreStartHookTerminalDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    collect_pre_start_hook_terminal(&dispatch.terminal, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

pub(in crate::runtime::kmes) fn collect_post_start_hook_terminal_dispatch(
    dispatch: &crate::supervisor::SupervisorPostStartHookTerminalDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    collect_post_start_hook_terminal(&dispatch.terminal, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

pub(in crate::runtime::kmes) fn collect_pre_start_hook_timeout(
    dispatch: &PreStartHookTimeoutDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)
}

pub(in crate::runtime::kmes) fn collect_post_start_hook_timeout(
    dispatch: &PostStartHookTimeoutDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)
}

pub(in crate::runtime::kmes) fn collect_readiness_timeout(
    dispatch: &ReadinessTimeoutDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    if let Some(job_event) = &dispatch.job_event {
        push_job(out, job_event)?;
    }
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)
}

pub(in crate::runtime::kmes) fn collect_service_main_start_timeout(
    dispatch: &ServiceMainStartTimeoutDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)
}

pub(in crate::runtime::kmes::job) fn collect_service_main_started(
    dispatch: &ServiceMainJobStartedDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)?;
    if let Some(job_event) = &dispatch.post_start_hook {
        push_job(out, job_event)?;
    }
    Ok(())
}

pub(in crate::runtime::kmes::job) fn collect_start_failure(
    dispatch: &StartFailureDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)
}

pub(in crate::runtime::kmes::job) fn collect_pre_start_hook_terminal(
    dispatch: &PreStartHookTerminalDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    if let Some(job_event) = &dispatch.next_job_event {
        push_job(out, job_event)?;
    }
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)
}

pub(in crate::runtime::kmes::job) fn collect_post_start_hook_terminal(
    dispatch: &PostStartHookTerminalDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)?;
    if let Some(job_event) = &dispatch.next_job_event {
        push_job(out, job_event)?;
    }
    push_operations(out, &dispatch.operation_events)?;
    push_graphs(out, &dispatch.graph_events)
}
