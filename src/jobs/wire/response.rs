use serde_json::{Value, json};

use super::model::JobsErrorCode;

/// The one success shape on the jobs channel: `{"status":"ok","job":{…}}`,
/// compact, with no terminator (PSPU §7.4).
pub fn jobs_job_response(view: Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&json!({
        "status": "ok",
        "job": view,
    }))
}

pub fn jobs_error_response(
    code: JobsErrorCode,
    message: &str,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&json!({
        "status": "error",
        "code": code.as_str(),
        "message": message,
    }))
}

/// A generic message for the codes that carry no more detail than the code.
pub fn jobs_error_response_message(code: JobsErrorCode) -> &'static str {
    match code {
        JobsErrorCode::MalformedRequest => "malformed jobs request",
        JobsErrorCode::RequestTooLarge => "jobs request too large",
        JobsErrorCode::InvalidCommand => "invalid jobs command",
        JobsErrorCode::InvalidArguments => "invalid jobs arguments",
        JobsErrorCode::UnknownJob => "unknown job",
        JobsErrorCode::AccessDenied => "access denied",
        JobsErrorCode::InvalidState => "command invalid for the job's state",
        JobsErrorCode::QuotaExceeded => "submitter quota exceeded",
        JobsErrorCode::BadToken => "attached token cannot be a job identity",
        JobsErrorCode::InternalError => "jobs request failed",
    }
}
