use super::{
    JobsCommand, JobsErrorCode, JobsRequestParseError, JobsWaitCondition, parse_jobs_request,
};

#[test]
fn submit_keeps_the_object_for_the_definition_parser() {
    let parsed = parse_jobs_request(br#"{"command":"submit","image_path":"/bin/true"}"#)
        .expect("parsed");
    assert_eq!(parsed.command, JobsCommand::Submit);
    assert_eq!(parsed.job_id, None);
    assert_eq!(
        parsed.submit.expect("object")["image_path"],
        serde_json::json!("/bin/true")
    );
}

#[test]
fn every_other_command_needs_a_job_id() {
    for command in ["status", "wait", "stop", "signal"] {
        let body = format!(r#"{{"command":"{command}"}}"#);
        assert_eq!(
            parse_jobs_request(body.as_bytes()),
            Err(JobsRequestParseError::InvalidArguments),
            "{command}"
        );
    }
}

#[test]
fn stop_defaults_to_waiting_and_wait_defaults_to_terminal() {
    let parsed = parse_jobs_request(br#"{"command":"stop","job_id":"x"}"#).expect("parsed");
    assert!(parsed.wait);
    let parsed =
        parse_jobs_request(br#"{"command":"wait","job_id":"x","for":"ready"}"#).expect("parsed");
    assert_eq!(parsed.wait_for, JobsWaitCondition::Ready);
    assert_eq!(
        parse_jobs_request(br#"{"command":"wait","job_id":"x","for":"done"}"#),
        Err(JobsRequestParseError::InvalidArguments)
    );
}

#[test]
fn signal_needs_an_integer() {
    let parsed =
        parse_jobs_request(br#"{"command":"signal","job_id":"x","signal":15}"#).expect("parsed");
    assert_eq!(parsed.signal, Some(15));
    assert_eq!(
        parse_jobs_request(br#"{"command":"signal","job_id":"x","signal":"TERM"}"#),
        Err(JobsRequestParseError::InvalidArguments)
    );
}

#[test]
fn malformed_and_unknown_are_distinct() {
    assert_eq!(
        parse_jobs_request(b""),
        Err(JobsRequestParseError::MalformedRequest)
    );
    assert_eq!(
        parse_jobs_request(b"[1]"),
        Err(JobsRequestParseError::MalformedRequest)
    );
    assert_eq!(
        parse_jobs_request(br#"{"command":"dance"}"#),
        Err(JobsRequestParseError::InvalidCommand)
    );
}

#[test]
fn error_codes_round_trip_and_only_too_large_closes() {
    for code in [
        JobsErrorCode::MalformedRequest,
        JobsErrorCode::RequestTooLarge,
        JobsErrorCode::InvalidCommand,
        JobsErrorCode::InvalidArguments,
        JobsErrorCode::UnknownJob,
        JobsErrorCode::AccessDenied,
        JobsErrorCode::InvalidState,
        JobsErrorCode::QuotaExceeded,
        JobsErrorCode::BadToken,
        JobsErrorCode::InternalError,
    ] {
        assert_eq!(JobsErrorCode::parse(code.as_str()), Some(code));
        assert_eq!(
            code.closes_connection(),
            code == JobsErrorCode::RequestTooLarge
        );
    }
}
