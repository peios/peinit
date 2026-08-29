use crate::control::wire::{ControlCommand, ParsedControlRequest, parse_control_request};
use crate::security::TokenSummary;
use crate::shutdown::ShutdownKind;

use super::{SystemShutdownCommandAdmissionError, admit_system_shutdown_command};

#[test]
fn admits_parsed_shutdown_command() {
    let parsed =
        parse_control_request(br#"{"command":"shutdown","type":"halt"}"#).expect("parsed shutdown");
    let caller = TokenSummary::requested_identity("admin");

    let request =
        admit_system_shutdown_command(&parsed, Some(caller.clone())).expect("shutdown command");

    assert_eq!(request.kind, ShutdownKind::Halt);
    assert_eq!(request.caller, Some(caller));
}

#[test]
fn rejects_non_shutdown_command() {
    let parsed =
        parse_control_request(br#"{"command":"start","service":"app"}"#).expect("parsed start");

    assert_eq!(
        admit_system_shutdown_command(&parsed, None),
        Err(SystemShutdownCommandAdmissionError::InvalidCommand),
    );
}

#[test]
fn rejects_shutdown_command_without_parsed_kind() {
    let parsed = ParsedControlRequest {
        job_id: None,
        job_filter: None,
        command: ControlCommand::Shutdown,
        service: None,
        wait: false,
        shutdown_kind: None,
        operation_id: None,
    };

    assert_eq!(
        admit_system_shutdown_command(&parsed, None),
        Err(SystemShutdownCommandAdmissionError::InvalidArguments),
    );
}
