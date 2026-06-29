use crate::boundary::{BoundaryError, KmesEvent};
use crate::control::lifecycle::OnDemandStartDispatch;
use crate::execution::graph::GraphExecutionEvent;
use crate::kmes::{
    encode_critical_failure_event, encode_graph_event, encode_job_event, encode_operation_event,
    encode_shutdown_abandoned_event,
};
use crate::operation::store::{OperationEvent, OperationRequestOutcome};
use crate::supervisor::{
    SupervisorShutdownAbandonedDispatch, SupervisorShutdownFinalizationDispatch,
};

pub(super) fn collect_operation_request_outcome(
    outcome: &OperationRequestOutcome,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_operations(out, &outcome.events)
}

pub(super) fn collect_on_demand_start(
    dispatch: &OnDemandStartDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_operations(out, &dispatch.events)
}

pub(super) fn push_job(
    out: &mut Vec<KmesEvent>,
    event: &crate::job::JobEvent,
) -> Result<(), BoundaryError> {
    out.push(encode_job_event(event)?);
    Ok(())
}

pub(super) fn push_operation(
    out: &mut Vec<KmesEvent>,
    event: &OperationEvent,
) -> Result<(), BoundaryError> {
    out.push(encode_operation_event(event)?);
    Ok(())
}

pub(super) fn push_operations(
    out: &mut Vec<KmesEvent>,
    events: &[OperationEvent],
) -> Result<(), BoundaryError> {
    for event in events {
        push_operation(out, event)?;
    }
    Ok(())
}

pub(super) fn push_graph(
    out: &mut Vec<KmesEvent>,
    event: &GraphExecutionEvent,
) -> Result<(), BoundaryError> {
    out.push(encode_graph_event(event)?);
    Ok(())
}

pub(super) fn push_graphs(
    out: &mut Vec<KmesEvent>,
    events: &[GraphExecutionEvent],
) -> Result<(), BoundaryError> {
    for event in events {
        push_graph(out, event)?;
    }
    Ok(())
}

pub(super) fn push_critical_failure(
    out: &mut Vec<KmesEvent>,
    service: &str,
    trigger: &str,
    observed_at_ns: Option<u64>,
    finalization: &SupervisorShutdownFinalizationDispatch,
) -> Result<(), BoundaryError> {
    out.push(encode_critical_failure_event(
        service,
        trigger,
        observed_at_ns,
        finalization,
    )?);
    Ok(())
}

pub(super) fn push_shutdown_abandoned(
    out: &mut Vec<KmesEvent>,
    abandoned: &SupervisorShutdownAbandonedDispatch,
) -> Result<(), BoundaryError> {
    out.push(encode_shutdown_abandoned_event(abandoned)?);
    Ok(())
}
