use crate::boundary::{ProcessController, ProcessSignal, ProcessTarget};
use crate::job::{JobEvent, JobExit, JobState};
use crate::operation::{OperationRecord, OperationSource, OperationType};
use crate::service::ServiceDefinition;
use crate::service::runtime::{
    ServiceState, ServiceStoppingTimeoutEvidence, ServiceTransition, TransitionCause,
};

use super::model::{
    ControlExecutionContext, ControlExecutionDetail, ControlExecutionDispatch,
    ControlExecutionError, ControlOperationKind, ControlOperationRequest, StopEscalationDispatch,
};
use super::store::{ControlExecutionStore, StopTimeoutDeadline};

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(super) fn begin_stop_like_operation<P>(
    context: &mut ControlExecutionContext<'_, P>,
    request: ControlOperationRequest,
    operation_type: OperationType,
    target: ProcessTarget,
    definition: &ServiceDefinition,
) -> Result<ControlExecutionDispatch, ControlExecutionError>
where
    P: ProcessController + ?Sized,
{
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_store = context.control_store.clone();
    let state = next_services
        .runtime(&target.service)
        .map(|runtime| runtime.state);
    let was_reloading = state == Some(ServiceState::Reloading);
    let already_stopping = state == Some(ServiceState::Stopping);

    let operation_event = next_operations
        .start_operation(request.operation_id, request.observed_at_ns)
        .map_err(ControlExecutionError::OperationStore)?;
    let operation = next_operations
        .get(request.operation_id)
        .ok_or(
            crate::operation::store::OperationStoreError::UnknownOperation {
                id: request.operation_id,
            },
        )
        .map_err(ControlExecutionError::OperationStore)?
        .clone();
    let stop_cause = stop_transition_cause(operation.source);

    let (service_transition, deadline_ns, detail) = if already_stopping {
        // §8.3: this stop aborted a running restart, whose stop leg is
        // already draining the service -- SIGTERM sent, deadline armed. The
        // service has no `Stopping -> Stopping` edge and needs no second
        // signal; the stop adopts the leg in flight, so the main process's
        // exit completes the stop instead of beginning the restart's start
        // leg. Transitioning again ended the runtime loop (PEI-824).
        let adopted = next_store.stop_timeout_for_service(&target.service);
        let deadline_ns = adopted.as_ref().map_or_else(
            || stop_deadline_ns(&operation, request.observed_at_ns, definition),
            |deadline| deadline.due_at_ns,
        );
        if let Some(adopted) = adopted {
            next_store.remove_stop_timeout(adopted.operation_id);
            next_store.take_restart_stop_leg(adopted.operation_id);
        }
        (
            None,
            deadline_ns,
            ControlExecutionDetail::StopAlreadyAcknowledged {
                target: target.clone(),
            },
        )
    } else {
        let service_transition = next_services
            .transition_service(
                &target.service,
                ServiceTransition {
                    to: ServiceState::Stopping,
                    cause: stop_cause,
                },
            )
            .map_err(ControlExecutionError::ServiceTable)?;
        let deadline_ns = stop_deadline_ns(&operation, request.observed_at_ns, definition);
        next_services
            .record_stopping_timeout(
                &target.service,
                ServiceStoppingTimeoutEvidence {
                    started_at_ns: request.observed_at_ns,
                    due_at_ns: deadline_ns,
                    cause: stop_cause,
                },
            )
            .map_err(ControlExecutionError::ServiceTable)?;
        let stopping_acknowledged = next_services
            .runtime(&target.service)
            .is_some_and(|runtime| runtime.stopping_acknowledged);
        if !stopping_acknowledged {
            context
                .controller
                .signal_main(&target, ProcessSignal::Sigterm)
                .map_err(ControlExecutionError::Boundary)?;
        }
        let detail = if stopping_acknowledged {
            ControlExecutionDetail::StopAlreadyAcknowledged {
                target: target.clone(),
            }
        } else {
            ControlExecutionDetail::Signal {
                target: target.clone(),
                signal: ProcessSignal::Sigterm,
            }
        };
        (Some(service_transition), deadline_ns, detail)
    };

    let mut cancelled_reload_jobs = Vec::new();
    if was_reloading {
        let cancelled_reload = next_store.cancel_reload_for_service(&target.service);
        for deadline in cancelled_reload.commands {
            context
                .controller
                .kill_cgroup(&deadline.cgroup_id)
                .map_err(ControlExecutionError::Boundary)?;
            // The command's job ends here, not when the killed process is
            // reaped: its terminal would otherwise route to the reload
            // completion, which transitions the service back to Active --
            // an edge a Stopping or Inactive service does not have, and an
            // InvalidTransition that ended the runtime loop (PEI-824).
            if let Some(job_event) =
                cancel_reload_command_job(&mut next_jobs, deadline.job_id, request.observed_at_ns)?
            {
                cancelled_reload_jobs.push(job_event);
            }
        }
    }
    next_store.record_stop_timeout(StopTimeoutDeadline {
        operation_id: request.operation_id,
        service: target.service.clone(),
        cgroup_id: target.cgroup_id.clone(),
        due_at_ns: deadline_ns,
    });
    if operation_type == OperationType::Restart {
        next_store.record_restart_stop_leg(request.operation_id, target.service.clone());
    }

    *context.services = next_services;
    *context.operations = next_operations;
    *context.jobs = next_jobs;
    *context.control_store = next_store;

    Ok(ControlExecutionDispatch {
        operation_id: request.operation_id,
        service: target.service.clone(),
        kind: if operation_type == OperationType::Restart {
            ControlOperationKind::RestartStopLeg
        } else {
            ControlOperationKind::Stop
        },
        operation_event,
        service_transition,
        detail,
        deadline_ns,
        cancelled_reload_jobs,
    })
}

fn cancel_reload_command_job(
    jobs: &mut crate::job::JobStore,
    job_id: crate::ids::JobId,
    now_ns: u64,
) -> Result<Option<JobEvent>, ControlExecutionError> {
    const REASON: &str = "reload command cancelled by stop";
    let Some(state) = jobs.get(job_id).map(|job| job.state) else {
        return Ok(None);
    };
    let event = match state {
        JobState::Created => jobs.fail_job_before_start(job_id, now_ns, REASON),
        JobState::Running => {
            jobs.fail_running_job(job_id, now_ns, Some(JobExit::Signal(9)), REASON)
        }
        _ => return Ok(None),
    }
    .map_err(ControlExecutionError::JobStore)?;
    Ok(Some(event))
}

pub fn escalate_due_stop(
    control_store: &mut ControlExecutionStore,
    controller: &mut (impl ProcessController + ?Sized),
    deadline: StopTimeoutDeadline,
    now_ns: u64,
) -> Result<StopEscalationDispatch, ControlExecutionError> {
    let mut next_store = control_store.clone();
    next_store.remove_stop_timeout(deadline.operation_id);
    controller
        .kill_cgroup(&deadline.cgroup_id)
        .map_err(ControlExecutionError::Boundary)?;
    *control_store = next_store;

    Ok(StopEscalationDispatch {
        operation_id: deadline.operation_id,
        service: deadline.service,
        cgroup_id: deadline.cgroup_id,
        escalated_at_ns: now_ns,
    })
}

fn stop_deadline_ns(
    operation: &OperationRecord,
    started_at_ns: u64,
    definition: &ServiceDefinition,
) -> u64 {
    let leg_deadline_ns =
        started_at_ns.saturating_add(definition.stop_timeout_secs.saturating_mul(NANOS_PER_SEC));
    if operation.operation_type == OperationType::Stop {
        operation
            .created_at_ns
            .saturating_add(definition.stop_timeout_secs.saturating_mul(NANOS_PER_SEC))
    } else {
        leg_deadline_ns
    }
}

fn stop_transition_cause(source: OperationSource) -> TransitionCause {
    match source {
        OperationSource::BindsToPropagation => TransitionCause::BindsToPropagation,
        OperationSource::ConflictResolution => TransitionCause::ConflictEviction,
        OperationSource::Shutdown => TransitionCause::ShutdownWave,
        _ => TransitionCause::ExplicitStop,
    }
}
