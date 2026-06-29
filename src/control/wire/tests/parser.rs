use super::support::*;

#[test]
fn parses_shutdown_request_with_required_type() {
    let request = parse_control_request(br#"{"command":"shutdown","type":"reboot"}"#)
        .expect("shutdown request");

    assert_eq!(
        request,
        ParsedControlRequest {
            command: ControlCommand::Shutdown,
            service: None,
            wait: false,
            shutdown_kind: Some(ShutdownKind::Reboot),
            operation_id: None,
        },
    );
}

#[test]
fn parses_all_shutdown_types() {
    assert_eq!(
        shutdown_kind(br#"{"command":"shutdown","type":"poweroff"}"#),
        ShutdownKind::Poweroff,
    );
    assert_eq!(
        shutdown_kind(br#"{"command":"shutdown","type":"reboot"}"#),
        ShutdownKind::Reboot,
    );
    assert_eq!(
        shutdown_kind(br#"{"command":"shutdown","type":"halt"}"#),
        ShutdownKind::Halt,
    );
}

#[test]
fn rejects_shutdown_without_valid_type() {
    assert_eq!(
        parse_control_request(br#"{"command":"shutdown"}"#),
        Err(ControlRequestParseError::InvalidArguments),
    );
    assert_eq!(
        parse_control_request(br#"{"command":"shutdown","type":"restart"}"#),
        Err(ControlRequestParseError::InvalidArguments),
    );
    assert_eq!(
        parse_control_request(br#"{"command":"shutdown","type":1}"#),
        Err(ControlRequestParseError::InvalidArguments),
    );
}

#[test]
fn applies_default_wait_semantics_and_accepts_wait_override() {
    assert!(
        parse_control_request(br#"{"command":"start","service":"app"}"#)
            .expect("start")
            .wait
    );
    assert!(
        !parse_control_request(br#"{"command":"reload","service":"app"}"#)
            .expect("reload")
            .wait
    );
    assert!(
        !parse_control_request(br#"{"command":"start","service":"app","wait":false}"#)
            .expect("start wait false")
            .wait
    );
}

#[test]
fn requires_service_for_service_commands() {
    assert_eq!(
        parse_control_request(br#"{"command":"start"}"#),
        Err(ControlRequestParseError::InvalidArguments),
    );
    assert_eq!(
        parse_control_request(br#"{"command":"status","service":"app"}"#)
            .expect("status")
            .service,
        Some("app".to_string()),
    );
}

#[test]
fn requires_operation_id_for_operation_status() {
    assert_eq!(
        parse_control_request(br#"{"command":"operation-status"}"#),
        Err(ControlRequestParseError::InvalidArguments),
    );
    assert_eq!(
        parse_control_request(
            br#"{"command":"operation-status","operation_id":"019723b1-5a13-7000-8000-000000000001"}"#,
        )
        .expect("operation status")
        .operation_id,
        Some("019723b1-5a13-7000-8000-000000000001".to_string()),
    );
}

#[test]
fn rejects_malformed_or_unknown_requests() {
    assert_eq!(
        parse_control_request(b"{"),
        Err(ControlRequestParseError::MalformedRequest),
    );
    assert_eq!(
        parse_control_request(br#"[]"#),
        Err(ControlRequestParseError::MalformedRequest),
    );
    assert_eq!(
        parse_control_request(br#"{"command":"power-cycle"}"#),
        Err(ControlRequestParseError::InvalidCommand),
    );
    assert_eq!(
        parse_control_request(br#"{"command":1}"#),
        Err(ControlRequestParseError::InvalidCommand),
    );
}

#[test]
fn rejects_non_bool_wait() {
    assert_eq!(
        parse_control_request(br#"{"command":"start","service":"app","wait":"yes"}"#),
        Err(ControlRequestParseError::InvalidArguments),
    );
}
