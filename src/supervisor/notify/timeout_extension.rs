use crate::execution::notify::{NotifyAppliedField, NotifyApplyDispatch};
use crate::operation::{OperationRecord, OperationState, OperationType};
use crate::service::runtime::ServiceState;
use crate::supervisor::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;
const NANOS_PER_USEC: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeoutExtensionError {
    InvalidValue { service: String, value: String },
    MissingDefinition { service: String },
}

pub(super) fn apply_transition_timeout_extensions(
    work: &mut SupervisorWork,
    notify: &NotifyApplyDispatch,
    observed_at_ns: u64,
) -> Result<(), TimeoutExtensionError> {
    // A shutdown is in progress, and this service's wave has already begun, so
    // its extension belongs to `apply_shutdown_timeout_extensions` — which
    // finds it by its entry in `stop_deadlines`.
    //
    // Returning here for *every* service during a shutdown was the bug. A
    // service in a later wave, still winding down a start or a reload when the
    // shutdown was requested, has no stop deadline yet — so the shutdown path
    // finds nothing and returns too, and its EXTEND_TIMEOUT_USEC was dropped by
    // both. That is the service most likely to need it: it was in the middle of
    // something when the shutdown began, and its start or reload phase kept its
    // original deadline while its own wave was still far off.
    if work.shutdown.is_some() && has_stop_deadline(work, &notify.sender.service) {
        return Ok(());
    }
    for field in &notify.applied_fields {
        let NotifyAppliedField::ExtendTimeoutUsec { value } = field else {
            continue;
        };
        apply_transition_timeout_extension(work, &notify.sender.service, value, observed_at_ns)?;
    }
    Ok(())
}

/// Whether this service's stop wave has begun.
fn has_stop_deadline(work: &SupervisorWork, service: &str) -> bool {
    work.shutdown.as_ref().is_some_and(|shutdown| {
        shutdown
            .stop_deadlines
            .iter()
            .any(|deadline| deadline.service == service)
    })
}

fn apply_transition_timeout_extension(
    work: &mut SupervisorWork,
    service: &str,
    value: &str,
    observed_at_ns: u64,
) -> Result<(), TimeoutExtensionError> {
    let Some(state) = work.services.runtime(service).map(|runtime| runtime.state) else {
        return Ok(());
    };
    match state {
        ServiceState::Starting => extend_start_timeout(work, service, value, observed_at_ns),
        ServiceState::Stopping => extend_stop_timeout(work, service, value, observed_at_ns),
        ServiceState::Reloading => extend_reload_timeout(work, service, value, observed_at_ns),
        _ => Ok(()),
    }
}

fn extend_start_timeout(
    work: &mut SupervisorWork,
    service: &str,
    value: &str,
    observed_at_ns: u64,
) -> Result<(), TimeoutExtensionError> {
    let requested_usec = parse_extension_usec(service, value)?;
    let Some(deadline) = work.start.readiness_deadline_for_service(service) else {
        return Ok(());
    };
    let Some(operation) = work.operations.get(deadline.operation_id) else {
        return Ok(());
    };
    let due_at_ns = clamped_deadline(
        work,
        service,
        operation,
        requested_usec,
        observed_at_ns,
        PhaseTimeout::Start,
    )?;
    work.start
        .extend_readiness_deadline(deadline.operation_id, due_at_ns);
    Ok(())
}

fn extend_stop_timeout(
    work: &mut SupervisorWork,
    service: &str,
    value: &str,
    observed_at_ns: u64,
) -> Result<(), TimeoutExtensionError> {
    let requested_usec = parse_extension_usec(service, value)?;
    let Some(deadline) = work.control.stop_timeout_for_service(service) else {
        return Ok(());
    };
    let Some(operation) = work.operations.get(deadline.operation_id) else {
        return Ok(());
    };
    let due_at_ns = clamped_deadline(
        work,
        service,
        operation,
        requested_usec,
        observed_at_ns,
        PhaseTimeout::Stop,
    )?;
    work.control
        .extend_stop_timeout(deadline.operation_id, due_at_ns);
    Ok(())
}

fn extend_reload_timeout(
    work: &mut SupervisorWork,
    service: &str,
    value: &str,
    observed_at_ns: u64,
) -> Result<(), TimeoutExtensionError> {
    let requested_usec = parse_extension_usec(service, value)?;
    let Some(operation) = current_running_reload_operation(work, service) else {
        return Ok(());
    };
    let operation_id = operation.id;
    let due_at_ns = clamped_deadline(
        work,
        service,
        operation,
        requested_usec,
        observed_at_ns,
        PhaseTimeout::Start,
    )?;
    if work
        .control
        .extend_reload_command_deadline(operation_id, due_at_ns)
        .is_none()
    {
        work.control
            .extend_reload_detection_deadline(operation_id, due_at_ns);
    }
    Ok(())
}

fn current_running_reload_operation<'a>(
    work: &'a SupervisorWork,
    service: &str,
) -> Option<&'a OperationRecord> {
    let operation = work.operations.current_for_service(service)?;
    (operation.operation_type == OperationType::Reload
        && operation.state == OperationState::Running)
        .then_some(operation)
}

fn parse_extension_usec(service: &str, value: &str) -> Result<u64, TimeoutExtensionError> {
    value
        .parse::<u64>()
        .map_err(|_| TimeoutExtensionError::InvalidValue {
            service: service.to_string(),
            value: value.to_string(),
        })
}

fn clamped_deadline(
    work: &SupervisorWork,
    service: &str,
    operation: &OperationRecord,
    requested_usec: u64,
    observed_at_ns: u64,
    phase: PhaseTimeout,
) -> Result<u64, TimeoutExtensionError> {
    let definition = work.services.definition(service).ok_or_else(|| {
        TimeoutExtensionError::MissingDefinition {
            service: service.to_string(),
        }
    })?;
    let cap_base_ns = if operation.operation_type == OperationType::Restart {
        operation.started_at_ns.unwrap_or(observed_at_ns)
    } else {
        operation.created_at_ns
    };
    let cap_ns = cap_base_ns.saturating_add(
        phase
            .base_timeout_secs(definition)
            .saturating_mul(4)
            .saturating_mul(NANOS_PER_SEC),
    );
    let requested_ns = observed_at_ns.saturating_add(requested_usec.saturating_mul(NANOS_PER_USEC));
    // During a shutdown the global deadline binds too. A service whose wave has
    // not begun still reaches this path, and no phase extension may outlive the
    // shutdown it is running inside — the shutdown path applies the same cap
    // for a service already in its wave.
    let global_cap_ns = work
        .shutdown
        .as_ref()
        .map(|shutdown| shutdown.global_deadline_ns)
        .unwrap_or(u64::MAX);
    Ok(requested_ns.min(cap_ns).min(global_cap_ns))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhaseTimeout {
    Start,
    Stop,
}

impl PhaseTimeout {
    fn base_timeout_secs(self, definition: &crate::service::ServiceDefinition) -> u64 {
        match self {
            Self::Start => definition.start_timeout_secs,
            Self::Stop => definition.stop_timeout_secs,
        }
    }
}
