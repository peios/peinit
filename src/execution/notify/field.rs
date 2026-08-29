use crate::execution::control::ControlExecutionStore;
use crate::execution::graph::GraphExecutionStore;
use crate::execution::start::StartExecutionStore;
use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::notify::NotifyField;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::ServiceState;

use super::error::unknown_service;
use super::model::{
    AuthenticatedNotifySender, NotifyAppliedField, NotifyApplyDispatch, NotifyApplyError,
};
use super::reload::{apply_reload_ready, apply_reloading};
use super::start::apply_start_ready;

pub(super) struct NotifyFieldContext<'a> {
    pub services: &'a mut ServiceTable,
    pub operations: &'a mut OperationStore,
    pub graph: &'a mut GraphExecutionStore,
    pub jobs: &'a mut JobStore,
    pub job_ids: &'a mut JobIdAllocator,
    pub start_store: &'a mut StartExecutionStore,
    pub control_store: &'a mut ControlExecutionStore,
    pub sender: &'a AuthenticatedNotifySender,
    pub observed_at_ns: u64,
    pub dispatch: &'a mut NotifyApplyDispatch,
}

pub(super) fn apply_notify_field(
    context: &mut NotifyFieldContext<'_>,
    field: &NotifyField,
) -> Result<(), NotifyApplyError> {
    match field {
        NotifyField::Ready => apply_ready(context)?,
        NotifyField::Reloading => apply_reloading(
            &*context.services,
            &*context.operations,
            &mut *context.control_store,
            context.sender,
            context.observed_at_ns,
            &mut *context.dispatch,
        )?,
        NotifyField::Status(text) => apply_status(
            &mut *context.services,
            context.sender,
            text,
            &mut *context.dispatch,
        )?,
        NotifyField::Stopping => apply_stopping(
            &mut *context.services,
            context.sender,
            &mut *context.dispatch,
        )?,
        other => record_advisory_field(other, &mut *context.dispatch),
    }
    Ok(())
}

fn apply_ready(context: &mut NotifyFieldContext<'_>) -> Result<(), NotifyApplyError> {
    match service_state(&*context.services, &context.sender.service)? {
        ServiceState::Starting => apply_start_ready(context)?,
        ServiceState::Reloading => apply_reload_ready(
            &mut *context.services,
            &mut *context.operations,
            &mut *context.control_store,
            context.sender,
            context.observed_at_ns,
            &mut *context.dispatch,
        )?,
        _ => {}
    }
    context
        .dispatch
        .applied_fields
        .push(NotifyAppliedField::Ready);
    Ok(())
}

fn apply_status(
    services: &mut ServiceTable,
    sender: &AuthenticatedNotifySender,
    text: &str,
    dispatch: &mut NotifyApplyDispatch,
) -> Result<(), NotifyApplyError> {
    services
        .update_status_text(&sender.service, text.to_string())
        .map_err(NotifyApplyError::ServiceTable)?;
    dispatch.applied_fields.push(NotifyAppliedField::Status {
        text: text.to_string(),
    });
    Ok(())
}

fn apply_stopping(
    services: &mut ServiceTable,
    sender: &AuthenticatedNotifySender,
    dispatch: &mut NotifyApplyDispatch,
) -> Result<(), NotifyApplyError> {
    services
        .acknowledge_stopping(&sender.service)
        .map_err(NotifyApplyError::ServiceTable)?;
    dispatch.applied_fields.push(NotifyAppliedField::Stopping);
    Ok(())
}

fn record_advisory_field(field: &NotifyField, dispatch: &mut NotifyApplyDispatch) {
    match field {
        NotifyField::Progress(value) => {
            dispatch.applied_fields.push(NotifyAppliedField::Progress {
                value: value.clone(),
            });
        }
        NotifyField::ProgressUnit(value) => {
            dispatch
                .applied_fields
                .push(NotifyAppliedField::ProgressUnit {
                    value: value.clone(),
                });
        }
        NotifyField::Errno(value) => dispatch.applied_fields.push(NotifyAppliedField::Errno {
            value: value.clone(),
        }),
        NotifyField::ExitStatus(value) => {
            dispatch
                .applied_fields
                .push(NotifyAppliedField::ExitStatus {
                    value: value.clone(),
                });
        }
        NotifyField::Watchdog => dispatch.applied_fields.push(NotifyAppliedField::Watchdog),
        NotifyField::WatchdogUsec(value) => {
            dispatch
                .applied_fields
                .push(NotifyAppliedField::WatchdogUsec {
                    value: value.clone(),
                });
        }
        NotifyField::ExtendTimeoutUsec(value) => {
            dispatch
                .applied_fields
                .push(NotifyAppliedField::ExtendTimeoutUsec {
                    value: value.clone(),
                });
        }
        NotifyField::FdStore => dispatch.applied_fields.push(NotifyAppliedField::FdStore),
        NotifyField::FdName(name) => dispatch
            .applied_fields
            .push(NotifyAppliedField::FdName { name: name.clone() }),
        NotifyField::FdStoreRemove => {
            dispatch
                .applied_fields
                .push(NotifyAppliedField::FdStoreRemove);
        }
        NotifyField::FdPoll(value) => dispatch.applied_fields.push(NotifyAppliedField::FdPoll {
            value: value.clone(),
        }),
        NotifyField::Ready
        | NotifyField::Reloading
        | NotifyField::Stopping
        | NotifyField::Status(_) => {}
    }
}

fn service_state(services: &ServiceTable, service: &str) -> Result<ServiceState, NotifyApplyError> {
    services
        .runtime(service)
        .map(|runtime| runtime.state)
        .ok_or_else(|| unknown_service(service))
}
