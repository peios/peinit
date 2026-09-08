use crate::operation::{OperationState, OperationType};
use crate::service::ServiceTable;
use crate::service::runtime::TransitionCause;
use crate::shutdown::{ShutdownError, ShutdownPlanError, ShutdownStopDeadline};

use super::super::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// The deadline for an already-stopping participant, and whether peinit had to
/// substitute one.
pub(super) struct RetainedStopDeadline {
    pub(super) deadline: ShutdownStopDeadline,
    /// Why the retained evidence could not substantiate a deadline, when it
    /// could not. The service is then given no graceful budget at all rather
    /// than a guessed one.
    pub(super) unsubstantiated: Option<&'static str>,
}

/// The `StopTimeout` an already-stopping service is entitled to, from the
/// evidence retained when it entered Stopping (§10.1).
///
/// peinit never guesses a deadline it cannot substantiate — a stop that began
/// before the shutdown keeps its own clock, and inventing a fresh budget here
/// would hand a service a second full `StopTimeout` it had already spent.
///
/// But failing *closed* is not the same as failing *loudly*. This used to
/// return an error, which propagated out of `begin_stop_wave` into
/// `begin_shutdown` and failed the whole shutdown command; raised on a later
/// wave it reached `run_turn` as a `RuntimeShutdownLoopError`, ended PID 1's
/// event loop mid-shutdown, and left the machine with some services stopped
/// and some not — worse than either finishing or not starting (PEI-349).
///
/// So the failure is isolated to the participant: no substantiated deadline
/// means no graceful budget, which is a deadline already due. The timeout scan
/// escalates that one service to SIGKILL on its wave and the shutdown carries
/// on.
pub(super) fn retained_stop_deadline(
    work: &SupervisorWork,
    service: &str,
    wave_index: usize,
    now_ns: u64,
) -> Result<RetainedStopDeadline, ShutdownError> {
    let cgroup_id = service_root_cgroup_id(&work.services, service)?;
    let substitute = |unsubstantiated: &'static str| RetainedStopDeadline {
        deadline: ShutdownStopDeadline {
            service: service.to_string(),
            cgroup_id: cgroup_id.clone(),
            started_at_ns: now_ns,
            due_at_ns: now_ns,
            wave: wave_index,
            operation_id: None,
        },
        unsubstantiated: Some(unsubstantiated),
    };

    if let Some(operation) = work
        .operations
        .current_for_service(service)
        .filter(|operation| {
            operation.state == OperationState::Running
                && matches!(
                    operation.operation_type,
                    OperationType::Stop | OperationType::Restart
                )
        })
    {
        let Some(retained) = work.control.stop_timeout(operation.id) else {
            return Ok(substitute(
                "no retained timeout for the in-flight stop operation",
            ));
        };
        let Some(started_at_ns) = operation.started_at_ns else {
            return Ok(substitute(
                "the in-flight stop operation records no start time",
            ));
        };

        return Ok(RetainedStopDeadline {
            deadline: ShutdownStopDeadline {
                service: service.to_string(),
                cgroup_id,
                started_at_ns,
                due_at_ns: retained.due_at_ns,
                wave: wave_index,
                operation_id: Some(operation.id),
            },
            unsubstantiated: None,
        });
    }

    let runtime = work.services.runtime(service).ok_or_else(|| {
        ShutdownError::Plan(ShutdownPlanError::MissingRuntime {
            service: service.to_string(),
        })
    })?;
    let Some(retained) = runtime.stopping_timeout.as_ref() else {
        return Ok(substitute("no retained stopping timeout"));
    };
    // Each of these is a distinct way for the evidence to have outlived or
    // contradicted the state it describes, and the operator gets told which.
    if runtime.state != crate::service::runtime::ServiceState::Stopping {
        return Ok(substitute(
            "retained timeout but the service is not Stopping",
        ));
    }
    if runtime.cause != Some(retained.cause) {
        return Ok(substitute(
            "retained timeout's cause does not match the service's",
        ));
    }
    if !retained_stop_cause(retained.cause) {
        return Ok(substitute("retained timeout's cause is not a stop cause"));
    }
    if retained.due_at_ns < retained.started_at_ns {
        return Ok(substitute("retained timeout is due before it started"));
    }

    Ok(RetainedStopDeadline {
        deadline: ShutdownStopDeadline {
            service: service.to_string(),
            cgroup_id,
            started_at_ns: retained.started_at_ns,
            due_at_ns: retained.due_at_ns,
            wave: wave_index,
            operation_id: None,
        },
        unsubstantiated: None,
    })
}

fn retained_stop_cause(cause: TransitionCause) -> bool {
    matches!(
        cause,
        TransitionCause::ExplicitStop
            | TransitionCause::ShutdownWave
            | TransitionCause::ConflictEviction
            | TransitionCause::BindsToPropagation
    )
}

pub(in crate::supervisor) fn service_root_cgroup_id(
    services: &ServiceTable,
    service: &str,
) -> Result<String, ShutdownError> {
    let runtime = services.runtime(service).ok_or_else(|| {
        ShutdownError::Plan(ShutdownPlanError::MissingRuntime {
            service: service.to_string(),
        })
    })?;
    Ok(crate::job::service_cgroup_root_path(
        service,
        runtime.cgroup_generation,
    ))
}

pub(super) fn stop_deadline_ns(
    services: &ServiceTable,
    service: &str,
    started_at_ns: u64,
) -> Result<u64, ShutdownError> {
    let definition = services.definition(service).ok_or_else(|| {
        ShutdownError::Plan(ShutdownPlanError::MissingDefinition {
            service: service.to_string(),
        })
    })?;
    Ok(started_at_ns.saturating_add(definition.stop_timeout_secs.saturating_mul(NANOS_PER_SEC)))
}
