use std::borrow::Cow;

use crate::control::system::{
    SystemAccess, SystemAccessCheckError, SystemAccessDenied, SystemShutdownCommandAdmissionError,
};
use crate::control::wire::{
    ControlErrorCode, ControlRequestParseError, control_client_error_message,
    control_error_response_line, control_system_ok_response_line,
};
use crate::shutdown::ShutdownError;
use crate::supervisor::{SupervisorError, SupervisorSystemShutdownDispatch};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorSystemShutdownControlBodyError {
    Parse(ControlRequestParseError),
    Admission(SystemShutdownCommandAdmissionError),
    Authorization(SystemAccessCheckError),
    AccessDenied(Box<SystemAccessDenied>),
    Supervisor(Box<SupervisorError>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorSystemShutdownControlBodyResponse {
    Accepted {
        response_line: Vec<u8>,
        dispatch: Box<SupervisorSystemShutdownDispatch>,
    },
    Rejected {
        response_line: Vec<u8>,
        error: SupervisorSystemShutdownControlBodyError,
    },
}

impl SupervisorSystemShutdownControlBodyError {
    pub(super) fn supervisor(error: SupervisorError) -> Self {
        Self::Supervisor(Box::new(error))
    }

    pub fn response_error(&self) -> (ControlErrorCode, Cow<'static, str>) {
        match self {
            Self::Parse(error) => {
                let code = ControlErrorCode::from(*error);
                (code, Cow::Borrowed(control_client_error_message(code)))
            }
            Self::Admission(error) => {
                let code = admission_error_code(*error);
                (code, Cow::Borrowed(control_client_error_message(code)))
            }
            Self::Authorization(_) => internal_control_error(),
            Self::AccessDenied(error) => (
                ControlErrorCode::AccessDenied,
                Cow::Owned(format!(
                    "caller lacks {} on peinit control",
                    system_access_label(error.desired_access),
                )),
            ),
            Self::Supervisor(error)
                if matches!(
                    error.as_ref(),
                    SupervisorError::Shutdown(ShutdownError::AlreadyInProgress { .. })
                ) =>
            {
                (
                    ControlErrorCode::InvalidState,
                    Cow::Borrowed("command rejected during shutdown"),
                )
            }
            Self::Supervisor(_) => internal_control_error(),
        }
    }
}

pub fn system_shutdown_control_response_line(
    result: Result<&SupervisorSystemShutdownDispatch, &SupervisorSystemShutdownControlBodyError>,
) -> Result<Vec<u8>, serde_json::Error> {
    match result {
        Ok(_) => control_system_ok_response_line(),
        Err(error) => {
            let (code, message) = error.response_error();
            control_error_response_line(code, &message)
        }
    }
}

fn admission_error_code(error: SystemShutdownCommandAdmissionError) -> ControlErrorCode {
    match error {
        SystemShutdownCommandAdmissionError::InvalidCommand => ControlErrorCode::InvalidCommand,
        SystemShutdownCommandAdmissionError::InvalidArguments => ControlErrorCode::InvalidArguments,
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
