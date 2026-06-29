use super::support::*;

#[test]
fn parses_command_vocabulary_exactly() {
    for (wire, command) in [
        ("start", ControlCommand::Start),
        ("stop", ControlCommand::Stop),
        ("restart", ControlCommand::Restart),
        ("reload", ControlCommand::Reload),
        ("reset", ControlCommand::Reset),
        ("status", ControlCommand::Status),
        ("list", ControlCommand::List),
        ("shutdown", ControlCommand::Shutdown),
        ("reload-config", ControlCommand::ReloadConfig),
        ("operation-status", ControlCommand::OperationStatus),
    ] {
        assert_eq!(ControlCommand::parse(wire), Some(command));
        assert_eq!(command.as_str(), wire);
    }
    assert_eq!(ControlCommand::parse("ReloadConfig"), None);
    assert_eq!(ControlCommand::parse("reload_config"), None);
}

#[test]
fn control_response_status_vocabulary_matches_wire_contract() {
    assert_eq!(
        ControlResponseStatus::parse("ok"),
        Some(ControlResponseStatus::Ok)
    );
    assert_eq!(
        ControlResponseStatus::parse("error"),
        Some(ControlResponseStatus::Error),
    );
    assert_eq!(ControlResponseStatus::Ok.as_str(), "ok");
    assert_eq!(ControlResponseStatus::Error.as_str(), "error");
    assert_eq!(ControlResponseStatus::parse("OK"), None);
}

#[test]
fn control_error_code_vocabulary_matches_wire_contract() {
    for (wire, code) in [
        ("ACCESS_DENIED", ControlErrorCode::AccessDenied),
        ("UNKNOWN_SERVICE", ControlErrorCode::UnknownService),
        ("UNKNOWN_OPERATION", ControlErrorCode::UnknownOperation),
        ("MALFORMED_REQUEST", ControlErrorCode::MalformedRequest),
        ("REQUEST_TOO_LARGE", ControlErrorCode::RequestTooLarge),
        ("INVALID_COMMAND", ControlErrorCode::InvalidCommand),
        ("INVALID_ARGUMENTS", ControlErrorCode::InvalidArguments),
        ("INVALID_STATE", ControlErrorCode::InvalidState),
        ("OPERATION_TIMEOUT", ControlErrorCode::OperationTimeout),
        ("INTERNAL_ERROR", ControlErrorCode::InternalError),
    ] {
        assert_eq!(ControlErrorCode::parse(wire), Some(code));
        assert_eq!(code.as_str(), wire);
    }
    assert_eq!(ControlErrorCode::parse("INVALID_ARGUMENT"), None);
}

#[test]
fn maps_parse_and_frame_errors_to_response_codes() {
    assert_eq!(
        ControlErrorCode::from(ControlRequestParseError::MalformedRequest),
        ControlErrorCode::MalformedRequest,
    );
    assert_eq!(
        ControlErrorCode::from(ControlRequestParseError::InvalidCommand),
        ControlErrorCode::InvalidCommand,
    );
    assert_eq!(
        ControlErrorCode::from(ControlRequestParseError::InvalidArguments),
        ControlErrorCode::InvalidArguments,
    );
    assert_eq!(
        ControlErrorCode::from(ControlFrameRejectReason::MalformedRequest),
        ControlErrorCode::MalformedRequest,
    );
    assert_eq!(
        ControlErrorCode::from(ControlFrameRejectReason::RequestTooLarge),
        ControlErrorCode::RequestTooLarge,
    );
}
