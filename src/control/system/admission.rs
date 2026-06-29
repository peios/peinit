use crate::control::wire::{ControlCommand, ParsedControlRequest};
use crate::security::TokenSummary;

use super::model::{SystemShutdownCommandAdmissionError, SystemShutdownCommandRequest};

pub fn admit_system_shutdown_command(
    request: &ParsedControlRequest,
    caller: Option<TokenSummary>,
) -> Result<SystemShutdownCommandRequest, SystemShutdownCommandAdmissionError> {
    if request.command != ControlCommand::Shutdown {
        return Err(SystemShutdownCommandAdmissionError::InvalidCommand);
    }
    let kind = request
        .shutdown_kind
        .ok_or(SystemShutdownCommandAdmissionError::InvalidArguments)?;

    Ok(SystemShutdownCommandRequest { kind, caller })
}
