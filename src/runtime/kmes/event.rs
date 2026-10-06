use crate::boundary::BoundaryError;
use crate::control::lifecycle::OnDemandStartDispatch;
use crate::execution::graph::GraphExecutionEvent;
use crate::kmes::types::{
    CRITICAL_SERVICE_FAILED, GRAPH_OPERATION_ENDED, INTERNAL_ERROR_CONTAINED, SERVICE_ABANDONED,
};
use crate::kmes::{
    EventCollector, encode_critical_failure_event, encode_graph_event, encode_job_event,
    encode_operation_event, encode_shutdown_abandoned_event, job_event_type, operation_event_type,
};
use crate::operation::store::{OperationEvent, OperationRequestOutcome};
use crate::supervisor::{
    CriticalRebootTrigger, SupervisorShutdownAbandonedDispatch,
    SupervisorShutdownFinalizationDispatch,
};

pub(super) fn collect_operation_request_outcome(
    outcome: &OperationRequestOutcome,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_operations(out, &outcome.events)
}

pub(super) fn collect_on_demand_start(
    dispatch: &OnDemandStartDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_operations(out, &dispatch.events)
}

/// A job event. A `peinit.job.ended` whose arguments had to be cut to fit
/// the ring says so itself, in `object.job.arguments-truncated` (PEI-1082).
pub(super) fn push_job(
    out: &mut EventCollector<'_>,
    event: &crate::job::JobEvent,
) -> Result<(), BoundaryError> {
    out.push(job_event_type(event), |time| encode_job_event(event, time))
}

pub(super) fn push_operation(
    out: &mut EventCollector<'_>,
    event: &OperationEvent,
) -> Result<(), BoundaryError> {
    out.push(operation_event_type(event), |_| encode_operation_event(event))
}

pub(super) fn push_operations(
    out: &mut EventCollector<'_>,
    events: &[OperationEvent],
) -> Result<(), BoundaryError> {
    for event in events {
        push_operation(out, event)?;
    }
    Ok(())
}

pub(super) fn push_graph(
    out: &mut EventCollector<'_>,
    event: &GraphExecutionEvent,
) -> Result<(), BoundaryError> {
    out.push(GRAPH_OPERATION_ENDED, |_| encode_graph_event(event))
}

pub(super) fn push_graphs(
    out: &mut EventCollector<'_>,
    events: &[GraphExecutionEvent],
) -> Result<(), BoundaryError> {
    for event in events {
        push_graph(out, event)?;
    }
    Ok(())
}

pub(super) fn push_critical_failure(
    out: &mut EventCollector<'_>,
    service: &str,
    trigger: CriticalRebootTrigger,
    finalization: &SupervisorShutdownFinalizationDispatch,
) -> Result<(), BoundaryError> {
    out.push(CRITICAL_SERVICE_FAILED, |_| {
        encode_critical_failure_event(service, trigger, finalization)
    })
}

/// The audit record of a contained internal error: the job it retired, the
/// operation it failed, the starts its reactions released, and the
/// `peinit.internal-error.contained` that says what peinit could not do
/// (PEI-1125).
pub(super) fn push_internal_error(
    out: &mut EventCollector<'_>,
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
    out.push(INTERNAL_ERROR_CONTAINED, |_| {
        crate::kmes::encode_service_internal_error_event(dispatch)
    })?;
    super::job::collect_start_dispatches(&dispatch.start_dispatches, out)
}

pub(super) fn push_shutdown_abandoned(
    out: &mut EventCollector<'_>,
    abandoned: &SupervisorShutdownAbandonedDispatch,
) -> Result<(), BoundaryError> {
    out.push(SERVICE_ABANDONED, |_| encode_shutdown_abandoned_event(abandoned))
}
