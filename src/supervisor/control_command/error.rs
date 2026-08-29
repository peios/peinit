use std::borrow::Cow;

use crate::control::lifecycle::LifecycleCommandError;
use crate::control::query::QueryError;
use crate::control::reload_config::ReloadConfigError;
use crate::control::service_security::{
    ServiceAccess, ServiceAccessCheckError, ServiceAccessDenied,
};
use crate::control::system::{SystemAccess, SystemAccessCheckError, SystemAccessDenied};
use crate::control::wire::{
    ControlErrorCode, ControlRequestParseError, control_client_error_message,
};
use crate::ids::{OperationId, OperationIdParseError};
use crate::operation::store::OperationStoreError;
use crate::shutdown::ShutdownError;
use crate::supervisor::SupervisorError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorControlCommandBodyError {
    Parse(ControlRequestParseError),
    InvalidArguments,
    OperationIdParse(OperationIdParseError),
    UnknownService { service: String },
    UnknownOperation { operation_id: OperationId },
    UnknownJob { job_id: crate::ids::JobId },
    Query(QueryError),
    SystemAuthorization(SystemAccessCheckError),
    ServiceAuthorization(ServiceAccessCheckError),
    SystemAccessDenied(Box<SystemAccessDenied>),
    ServiceAccessDenied(Box<ServiceAccessDenied>),
    JobAuthorization(crate::submitted::JobAccessCheckError),
    JobAccessDenied(Box<crate::submitted::JobAccessDenied>),
    Supervisor(Box<SupervisorError>),
    ReloadConfig(Box<ReloadConfigError>),
    RegistryUnavailable,
    MissingLifecycleOperation,
    ResponseSerialize(String),
}

impl SupervisorControlCommandBodyError {
    pub(in crate::supervisor) fn serialize(error: serde_json::Error) -> Self {
        Self::ResponseSerialize(error.to_string())
    }

    pub(in crate::supervisor) fn supervisor(error: SupervisorError) -> Self {
        Self::Supervisor(Box::new(error))
    }

    pub(super) fn reload_config(error: ReloadConfigError) -> Self {
        Self::ReloadConfig(Box::new(error))
    }

    pub fn response_error(&self) -> (ControlErrorCode, Cow<'static, str>) {
        match self {
            Self::Parse(error) => {
                let code = ControlErrorCode::from(*error);
                (code, Cow::Borrowed(control_client_error_message(code)))
            }
            Self::InvalidArguments | Self::OperationIdParse(_) => (
                ControlErrorCode::InvalidArguments,
                Cow::Borrowed(control_client_error_message(
                    ControlErrorCode::InvalidArguments,
                )),
            ),
            Self::UnknownService { service } => (
                ControlErrorCode::UnknownService,
                Cow::Owned(format!("unknown service {service}")),
            ),
            Self::UnknownOperation { operation_id } => (
                ControlErrorCode::UnknownOperation,
                Cow::Owned(format!("unknown operation {operation_id}")),
            ),
            Self::UnknownJob { job_id } => (
                ControlErrorCode::UnknownJob,
                Cow::Owned(format!("unknown job {job_id}")),
            ),
            Self::JobAuthorization(_) => internal_control_error(),
            Self::JobAccessDenied(error) => (
                ControlErrorCode::AccessDenied,
                Cow::Owned(format!(
                    "caller lacks {} on job {}",
                    error.desired_access.label(),
                    error.job_id,
                )),
            ),
            Self::Query(QueryError::UnknownService { service }) => (
                ControlErrorCode::UnknownService,
                Cow::Owned(format!("unknown service {service}")),
            ),
            Self::Query(QueryError::UnknownOperation { operation_id }) => (
                ControlErrorCode::UnknownOperation,
                Cow::Owned(format!("unknown operation {operation_id}")),
            ),
            Self::Query(QueryError::MissingCurrentJobRecord { .. }) => internal_control_error(),
            Self::SystemAuthorization(_) | Self::ServiceAuthorization(_) => {
                internal_control_error()
            }
            Self::SystemAccessDenied(error) => (
                ControlErrorCode::AccessDenied,
                Cow::Owned(format!(
                    "caller lacks {} on peinit control",
                    system_access_label(error.desired_access),
                )),
            ),
            Self::ServiceAccessDenied(error) => (
                ControlErrorCode::AccessDenied,
                Cow::Owned(format!(
                    "caller lacks {} on {}",
                    service_access_label(error.desired_access),
                    error.service,
                )),
            ),
            Self::Supervisor(error) => supervisor_error_response(error.as_ref()),
            Self::ReloadConfig(error) => reload_config_error_response(error.as_ref()),
            Self::RegistryUnavailable
            | Self::MissingLifecycleOperation
            | Self::ResponseSerialize(_) => internal_control_error(),
        }
    }
}

fn supervisor_error_response(error: &SupervisorError) -> (ControlErrorCode, Cow<'static, str>) {
    match error {
        SupervisorError::Lifecycle(error) => lifecycle_error_response(error),
        SupervisorError::Shutdown(ShutdownError::AlreadyInProgress { .. }) => (
            ControlErrorCode::InvalidState,
            Cow::Borrowed("command rejected during shutdown"),
        ),
        _ => internal_control_error(),
    }
}

fn lifecycle_error_response(
    error: &LifecycleCommandError,
) -> (ControlErrorCode, Cow<'static, str>) {
    match error {
        LifecycleCommandError::UnknownService { service }
        | LifecycleCommandError::DefinitionRemoved { service } => (
            ControlErrorCode::UnknownService,
            Cow::Owned(format!("unknown service {service}")),
        ),
        LifecycleCommandError::InvalidState {
            service,
            command,
            state,
        } => (
            ControlErrorCode::InvalidState,
            Cow::Owned(format!(
                "{:?} is invalid for service {service} in {:?}",
                command, state
            )),
        ),
        LifecycleCommandError::OperationStore(OperationStoreError::ConflictRejected(rejection)) => {
            (
                ControlErrorCode::InvalidState,
                Cow::Owned(format!("operation conflict rejected: {rejection:?}")),
            )
        }
        LifecycleCommandError::StartPlan(error) => (
            ControlErrorCode::InvalidState,
            Cow::Owned(format!("start plan rejected: {error:?}")),
        ),
        _ => internal_control_error(),
    }
}

fn reload_config_error_response(
    error: &ReloadConfigError,
) -> (ControlErrorCode, Cow<'static, str>) {
    match error {
        ReloadConfigError::Validation(failure) => (
            ControlErrorCode::InvalidState,
            Cow::Owned(format!(
                "configuration validation failed: {:?}",
                failure.findings
            )),
        ),
        ReloadConfigError::Registry(_) | ReloadConfigError::ServiceTable(_) => {
            internal_control_error()
        }
    }
}

fn internal_control_error() -> (ControlErrorCode, Cow<'static, str>) {
    (
        ControlErrorCode::InternalError,
        Cow::Borrowed(control_client_error_message(
            ControlErrorCode::InternalError,
        )),
    )
}

fn system_access_label(access: SystemAccess) -> &'static str {
    match access.bits() {
        0x0001 => "SYSTEM_SHUTDOWN",
        0x0002 => "SYSTEM_RELOAD_CONFIG",
        _ => "SYSTEM_ACCESS",
    }
}

fn service_access_label(access: ServiceAccess) -> &'static str {
    match access.bits() {
        0x0001 => "SERVICE_QUERY_STATUS",
        0x0002 => "SERVICE_START",
        0x0004 => "SERVICE_STOP",
        0x0008 => "SERVICE_INTERROGATE",
        0x0006 => "SERVICE_START|SERVICE_STOP",
        0x000f => "SERVICE_ALL",
        _ => "SERVICE_ACCESS",
    }
}
