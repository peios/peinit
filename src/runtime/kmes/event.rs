use crate::boundary::{BoundaryError, KmesEvent};
use crate::control::lifecycle::OnDemandStartDispatch;
use crate::execution::graph::GraphExecutionEvent;
use crate::kmes::{
    encode_critical_failure_event, encode_graph_event, encode_job_event_bounded,
    encode_operation_event, encode_shutdown_abandoned_event,
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

/// A job event, and — when a `job.ended` had its arguments cut to fit the
/// ring — the `event.oversized` that records the cut beside it (PEI-1082).
pub(super) fn push_job(
    out: &mut Vec<KmesEvent>,
    event: &crate::job::JobEvent,
) -> Result<(), BoundaryError> {
    let (encoded, truncation) = encode_job_event_bounded(event)?;
    let event_type = encoded.event_type.clone();
    out.push(encoded);
    if let Some(truncation) = truncation {
        let job_id = event.job_id.to_string();
        out.push(crate::kmes::encode_event_oversized_event(
            &crate::kmes::OversizedEvent {
                event_type: &event_type,
                action: crate::kmes::OversizedEventAction::Truncated,
                service: event.service.as_deref(),
                job_id: Some(&job_id),
                size_bytes: truncation.arguments_bytes,
                limit_bytes: Some(truncation.limit_bytes),
                dropped_total: None,
                error: None,
            },
        )?);
    }
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

/// The audit record of a contained internal error: the job it retired, the
/// operation it failed, the starts its reactions released, and the
/// `service.internal_error` that says what peinit could not do (PEI-1125).
pub(super) fn push_internal_error(
    out: &mut Vec<KmesEvent>,
    dispatch: &crate::supervisor::SupervisorInternalErrorDispatch,
) -> Result<(), BoundaryError> {
    if let Some(job_event) = &dispatch.job_event {
        push_job(out, job_event)?;
    }
    if let Some(job_event) = &dispatch.service_job_event {
        push_job(out, job_event)?;
    }
    if let Some(operation_event) = &dispatch.operation_event {
        push_operation(out, operation_event)?;
    }
    out.push(crate::kmes::encode_service_internal_error_event(dispatch)?);
    super::job::collect_start_dispatches(&dispatch.start_dispatches, out)
}

pub(super) fn push_shutdown_abandoned(
    out: &mut Vec<KmesEvent>,
    abandoned: &SupervisorShutdownAbandonedDispatch,
) -> Result<(), BoundaryError> {
    out.push(encode_shutdown_abandoned_event(abandoned)?);
    Ok(())
}
