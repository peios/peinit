use serde_json::Value;

use crate::shutdown::ShutdownKind;

use super::model::{ControlCommand, ControlRequestParseError, ParsedControlRequest};

pub fn parse_control_request(
    body: &[u8],
) -> Result<ParsedControlRequest, ControlRequestParseError> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| ControlRequestParseError::MalformedRequest)?;
    let object = value
        .as_object()
        .ok_or(ControlRequestParseError::MalformedRequest)?;

    let command_value = object
        .get("command")
        .and_then(Value::as_str)
        .ok_or(ControlRequestParseError::InvalidCommand)?;
    let command =
        ControlCommand::parse(command_value).ok_or(ControlRequestParseError::InvalidCommand)?;
    let wait = match object.get("wait") {
        Some(value) => value
            .as_bool()
            .ok_or(ControlRequestParseError::InvalidArguments)?,
        None => command.default_wait(),
    };
    let service = if command_requires_service(command) {
        Some(required_string_field(object, "service")?.to_string())
    } else {
        object
            .get("service")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    };
    let shutdown_kind = if command == ControlCommand::Shutdown {
        Some(parse_shutdown_kind(required_string_field(object, "type")?)?)
    } else {
        None
    };
    let operation_id = if command == ControlCommand::OperationStatus {
        Some(required_string_field(object, "operation_id")?.to_string())
    } else {
        None
    };
    let job_id = if matches!(command, ControlCommand::JobStatus | ControlCommand::JobStop) {
        Some(required_string_field(object, "job_id")?.to_string())
    } else {
        None
    };
    let job_filter = if command == ControlCommand::JobList {
        Some(parse_job_filter(object)?)
    } else {
        None
    };

    Ok(ParsedControlRequest {
        command,
        service,
        wait,
        shutdown_kind,
        operation_id,
        job_id,
        job_filter,
    })
}

fn command_requires_service(command: ControlCommand) -> bool {
    matches!(
        command,
        ControlCommand::Start
            | ControlCommand::Stop
            | ControlCommand::Restart
            | ControlCommand::Reload
            | ControlCommand::Reset
            | ControlCommand::Status
    )
}

fn required_string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    field: &str,
) -> Result<&'a str, ControlRequestParseError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or(ControlRequestParseError::InvalidArguments)
}

/// The four `job-list` filters (PSPU §4.8): each optional, each refused
/// when present and malformed rather than matched against nothing.
fn parse_job_filter(
    object: &serde_json::Map<String, Value>,
) -> Result<crate::submitted::SubmittedJobListFilter, ControlRequestParseError> {
    let sid_filter = |field: &str| -> Result<Option<String>, ControlRequestParseError> {
        match object.get(field) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(sid)) if is_sid_string(sid) => Ok(Some(sid.clone())),
            Some(_) => Err(ControlRequestParseError::InvalidArguments),
        }
    };
    let logon_session = match object.get("logon_session") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or(ControlRequestParseError::InvalidArguments)?,
        ),
    };
    let state = match object.get("state") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .and_then(crate::submitted::parse_job_state_wire)
                .ok_or(ControlRequestParseError::InvalidArguments)?,
        ),
    };
    Ok(crate::submitted::SubmittedJobListFilter {
        submitter_sid: sid_filter("submitter")?,
        identity_sid: sid_filter("identity")?,
        logon_session,
        state,
    })
}

fn is_sid_string(value: &str) -> bool {
    let mut parts = value.split('-');
    parts.next() == Some("S")
        && parts.next() == Some("1")
        && parts.clone().count() >= 1
        && parts.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn parse_shutdown_kind(value: &str) -> Result<ShutdownKind, ControlRequestParseError> {
    match value {
        "poweroff" => Ok(ShutdownKind::Poweroff),
        "reboot" => Ok(ShutdownKind::Reboot),
        "halt" => Ok(ShutdownKind::Halt),
        _ => Err(ControlRequestParseError::InvalidArguments),
    }
}
