use std::borrow::Cow;

use crate::boundary::BoundaryError;
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
        // A read the registry refused as a whole — the Services root that
        // will not open, say. A definition that will not decode is no longer
        // one of these (PEI-621: it fails that service and the reload is
        // accepted), but the registry's own message still says more than
        // "control request failed" (TRM §10.4), so hand it on. The code stays
        // INTERNAL_ERROR: the vocabulary (PSPU §4.10) has nothing closer, since
        // INVALID_STATE is about the service's state or a shutdown, and
        // UNKNOWN_SERVICE would say the service does not exist when it does.
        ReloadConfigError::Registry(BoundaryError::Registry(message)) => (
            ControlErrorCode::InternalError,
            Cow::Owned(format!("configuration reload failed: {message}")),
        ),
        ReloadConfigError::Registry(error) => (
            ControlErrorCode::InternalError,
            Cow::Owned(format!("configuration reload failed: {error:?}")),
        ),
        ReloadConfigError::ServiceTable(error) => (
            ControlErrorCode::InternalError,
            Cow::Owned(format!(
                "configuration reload failed to apply the service table: {error:?}"
            )),
        ),
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

#[cfg(test)]
mod tests {
    use crate::boundary::BoundaryError;
    use crate::control::reload_config::ReloadConfigError;
    use crate::control::wire::ControlErrorCode;
    use crate::service::ServiceTableError;

    use super::SupervisorControlCommandBodyError;

    /// TRM §10.4: a reload the registry refused answers with the registry's
    /// own message, not "control request failed" (PEI-1075). An undecodable
    /// key no longer refuses the reload at all (PEI-621); this is the read
    /// that fails as a whole — the Services root that will not open.
    #[test]
    fn reload_config_registry_error_carries_the_registry_message() {
        let error =
            SupervisorControlCommandBodyError::ReloadConfig(Box::new(ReloadConfigError::Registry(
                BoundaryError::Registry("OpenRoot(Os { code: 2, kind: NotFound })".to_string()),
            )));

        let (code, message) = error.response_error();

        assert_eq!(code, ControlErrorCode::InternalError);
        assert_eq!(
            message,
            "configuration reload failed: OpenRoot(Os { code: 2, kind: NotFound })"
        );
    }

    #[test]
    fn reload_config_service_table_error_keeps_its_detail() {
        let error = SupervisorControlCommandBodyError::ReloadConfig(Box::new(
            ReloadConfigError::ServiceTable(ServiceTableError::DuplicateService {
                service: "app".to_string(),
            }),
        ));

        let (code, message) = error.response_error();

        assert_eq!(code, ControlErrorCode::InternalError);
        assert!(message.contains("DuplicateService"), "{message}");
        assert!(message.contains("app"), "{message}");
    }
}
