use crate::ids::JobId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl LogStream {
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Stderr)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceLogRecord {
    pub origin: String,
    pub is_error: bool,
    pub message: String,
    pub timestamp_ns: u64,
    pub job_id: Option<JobId>,
}

impl ServiceLogRecord {
    pub fn new(
        origin: impl Into<String>,
        stream: LogStream,
        message: impl Into<String>,
        timestamp_ns: u64,
        job_id: Option<JobId>,
    ) -> Self {
        Self {
            origin: origin.into(),
            is_error: stream.is_error(),
            message: message.into(),
            timestamp_ns,
            job_id,
        }
    }
}
