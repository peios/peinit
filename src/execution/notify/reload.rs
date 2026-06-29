use crate::execution::control::ControlExecutionStore;
use crate::operation::store::OperationStore;
use crate::operation::{OperationRecord, OperationState, OperationType};
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::error::unknown_service;
use super::model::{
    AuthenticatedNotifySender, NotifyAppliedField, NotifyApplyDispatch, NotifyApplyError,
};

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(super) fn apply_reload_ready(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    control_store: &mut ControlExecutionStore,
    sender: &AuthenticatedNotifySender,
    observed_at_ns: u64,
    dispatch: &mut NotifyApplyDispatch,
) -> Result<(), NotifyApplyError> {
    let operation_id = reload_operation(operations, &sender.service)?.id;
    if control_store.has_reload_command_deadline(operation_id) {
        control_store.mark_reload_command_ready(operation_id);
        return Ok(());
    }
    control_store.remove_reload_detection_deadline(operation_id);
    let transition = services
        .transition_service(
            &sender.service,
            ServiceTransition {
                to: ServiceState::Active,
                cause: TransitionCause::ExplicitReload,
            },
        )
        .map_err(NotifyApplyError::ServiceTable)?;
    let operation_event = operations
        .complete_operation(operation_id, observed_at_ns, "reload signal confirmed")
        .map_err(NotifyApplyError::OperationStore)?;
    dispatch.service_transitions.push(transition);
    dispatch.operation_events.push(operation_event);
    Ok(())
}

pub(super) fn apply_reloading(
    services: &ServiceTable,
    operations: &OperationStore,
    control_store: &mut ControlExecutionStore,
    sender: &AuthenticatedNotifySender,
    observed_at_ns: u64,
    dispatch: &mut NotifyApplyDispatch,
) -> Result<(), NotifyApplyError> {
    if service_state(services, &sender.service)? != ServiceState::Reloading {
        dispatch.applied_fields.push(NotifyAppliedField::Reloading);
        return Ok(());
    }
    let operation = reload_operation(operations, &sender.service)?;
    if !control_store.has_reload_command_deadline(operation.id) {
        let definition = services
            .definition(&sender.service)
            .ok_or_else(|| unknown_service(&sender.service))?;
        control_store.observe_reload_reloading(
            operation.id,
            observed_at_ns
                .saturating_add(definition.start_timeout_secs.saturating_mul(NANOS_PER_SEC)),
        );
    }
    dispatch.applied_fields.push(NotifyAppliedField::Reloading);
    Ok(())
}

fn reload_operation<'a>(
    operations: &'a OperationStore,
    service: &str,
) -> Result<&'a OperationRecord, NotifyApplyError> {
    let operation = operations.current_for_service(service).ok_or_else(|| {
        NotifyApplyError::MissingReloadOperation {
            service: service.to_string(),
        }
    })?;
    if operation.operation_type != OperationType::Reload
        || operation.state != OperationState::Running
    {
        return Err(NotifyApplyError::UnsupportedReloadOperation {
            service: service.to_string(),
            operation_type: operation.operation_type,
        });
    }
    Ok(operation)
}

fn service_state(services: &ServiceTable, service: &str) -> Result<ServiceState, NotifyApplyError> {
    services
        .runtime(service)
        .map(|runtime| runtime.state)
        .ok_or_else(|| unknown_service(service))
}
