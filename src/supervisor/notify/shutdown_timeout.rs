use crate::execution::notify::{NotifyAppliedField, NotifyApplyDispatch};
use crate::shutdown::ShutdownError;

use crate::supervisor::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;
const NANOS_PER_USEC: u64 = 1_000;

pub(super) fn apply_shutdown_timeout_extensions(
    work: &mut SupervisorWork,
    notify: &NotifyApplyDispatch,
    observed_at_ns: u64,
) -> Result<(), ShutdownError> {
    for field in &notify.applied_fields {
        let NotifyAppliedField::ExtendTimeoutUsec { value } = field else {
            continue;
        };
        extend_shutdown_stop_deadline(work, &notify.sender.service, value, observed_at_ns)?;
    }
    Ok(())
}

fn extend_shutdown_stop_deadline(
    work: &mut SupervisorWork,
    service: &str,
    value: &str,
    observed_at_ns: u64,
) -> Result<(), ShutdownError> {
    if work.shutdown.is_none() {
        return Ok(());
    }
    let Some(deadline) = work.shutdown.as_ref().and_then(|shutdown| {
        shutdown
            .stop_deadlines
            .iter()
            .find(|d| d.service == service)
    }) else {
        return Ok(());
    };
    let requested_extension_usec =
        value
            .parse::<u64>()
            .map_err(|_| ShutdownError::InvalidTimeoutExtension {
                service: service.to_string(),
                value: value.to_string(),
            })?;
    let stop_timeout_secs = work
        .services
        .definition(service)
        .ok_or_else(|| {
            ShutdownError::Plan(crate::shutdown::ShutdownPlanError::MissingDefinition {
                service: service.to_string(),
            })
        })?
        .stop_timeout_secs;
    let started_at_ns = deadline.started_at_ns;
    let operation_id = deadline.operation_id;
    let global_deadline_ns = work.shutdown()?.global_deadline_ns;
    let requested_deadline_ns =
        observed_at_ns.saturating_add(requested_extension_usec.saturating_mul(NANOS_PER_USEC));
    let service_cap_ns = started_at_ns.saturating_add(
        stop_timeout_secs
            .saturating_mul(4)
            .saturating_mul(NANOS_PER_SEC),
    );
    let extended_deadline_ns = requested_deadline_ns
        .min(service_cap_ns)
        .min(global_deadline_ns);

    if let Some(deadline) = work
        .shutdown_mut()?
        .stop_deadlines
        .iter_mut()
        .find(|deadline| deadline.service == service)
    {
        deadline.due_at_ns = extended_deadline_ns;
    }
    if let Some(operation_id) = operation_id {
        work.control
            .extend_stop_timeout(operation_id, extended_deadline_ns);
    }
    Ok(())
}
