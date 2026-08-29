use serde_json::Value;

use super::model::{JobsCommand, JobsRequestParseError, JobsWaitCondition, ParsedJobsRequest};

/// Parse one message's content. The definition inside a `submit` is left
/// as the object for `crate::submitted` to validate against the attached
/// descriptors, which this parser never sees.
pub fn parse_jobs_request(body: &[u8]) -> Result<ParsedJobsRequest, JobsRequestParseError> {
    if body.is_empty() {
        return Err(JobsRequestParseError::MalformedRequest);
    }
    let value: Value =
        serde_json::from_slice(body).map_err(|_| JobsRequestParseError::MalformedRequest)?;
    let object = value
        .as_object()
        .ok_or(JobsRequestParseError::MalformedRequest)?;

    let command_value = object
        .get("command")
        .and_then(Value::as_str)
        .ok_or(JobsRequestParseError::InvalidCommand)?;
    let command =
        JobsCommand::parse(command_value).ok_or(JobsRequestParseError::InvalidCommand)?;

    let job_id = if command == JobsCommand::Submit {
        None
    } else {
        Some(
            object
                .get("job_id")
                .and_then(Value::as_str)
                .ok_or(JobsRequestParseError::InvalidArguments)?
                .to_string(),
        )
    };
    let wait = match object.get("wait") {
        None | Some(Value::Null) => true,
        Some(value) => value
            .as_bool()
            .ok_or(JobsRequestParseError::InvalidArguments)?,
    };
    let wait_for = match object.get("for") {
        None | Some(Value::Null) => JobsWaitCondition::Terminal,
        Some(value) => match value.as_str() {
            Some("terminal") => JobsWaitCondition::Terminal,
            Some("ready") => JobsWaitCondition::Ready,
            _ => return Err(JobsRequestParseError::InvalidArguments),
        },
    };
    let signal = if command == JobsCommand::Signal {
        let signal = object
            .get("signal")
            .and_then(Value::as_i64)
            .ok_or(JobsRequestParseError::InvalidArguments)?;
        Some(i32::try_from(signal).map_err(|_| JobsRequestParseError::InvalidArguments)?)
    } else {
        None
    };
    let submit = (command == JobsCommand::Submit).then(|| object.clone());

    Ok(ParsedJobsRequest {
        command,
        job_id,
        wait,
        wait_for,
        signal,
        submit,
    })
}
