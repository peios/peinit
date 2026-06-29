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

    Ok(ParsedControlRequest {
        command,
        service,
        wait,
        shutdown_kind,
        operation_id,
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

fn parse_shutdown_kind(value: &str) -> Result<ShutdownKind, ControlRequestParseError> {
    match value {
        "poweroff" => Ok(ShutdownKind::Poweroff),
        "reboot" => Ok(ShutdownKind::Reboot),
        "halt" => Ok(ShutdownKind::Halt),
        _ => Err(ControlRequestParseError::InvalidArguments),
    }
}
