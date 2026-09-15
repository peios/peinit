use std::io;
use std::os::fd::OwnedFd;
use std::path::PathBuf;

use crate::control::socket::{ControlSocketBindError, ControlSocketPathError};

pub const JOBS_SOCKET_PATH: &str = "/run/services/peinit/jobs.sock";
pub const JOBS_SOCKET_LISTEN_BACKLOG: i32 = 32;
pub const DEFAULT_MAX_JOBS_CONNECTIONS: usize = 64;
/// Half of KMES's default `MaxEventSize` (65536), and at most the
/// `job.ended` argument budget (`kmes::MAX_JOB_ENDED_ARGUMENTS_BYTES`).
///
/// A submission's `arguments` come back out in its `job.ended`, next to
/// about twenty other fields, and the two limits used to be equal — so a
/// record that filled one message produced an event the ring refused, and
/// until PEI-1125 that ended PID 1's runtime loop (PEI-1082). The event's
/// arguments are cut to their budget regardless of this setting; this
/// default is what keeps a default-sized record from ever being cut, with
/// the other half of the event left for the record's remaining fields.
pub const DEFAULT_MAX_JOBS_MESSAGE_BYTES: usize = 32_768;
pub const DEFAULT_JOBS_CONNECTION_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_MAX_JOBS_PER_SUBMITTER: usize = 64;
/// Descriptors accepted on one message, the output sink included (PSPU §7.A).
pub const MAX_JOBS_MESSAGE_DESCRIPTORS: usize = 64;

/// The configurable bounds of PSPU §7.3, read from `Machine\System\Init\`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobsSocketLimits {
    pub max_connections: usize,
    pub max_message_bytes: usize,
    pub connection_timeout_secs: u64,
    pub max_jobs_per_submitter: usize,
}

impl Default for JobsSocketLimits {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_JOBS_CONNECTIONS,
            max_message_bytes: DEFAULT_MAX_JOBS_MESSAGE_BYTES,
            connection_timeout_secs: DEFAULT_JOBS_CONNECTION_TIMEOUT_SECS,
            max_jobs_per_submitter: DEFAULT_MAX_JOBS_PER_SUBMITTER,
        }
    }
}

#[derive(Debug)]
pub enum JobsSocketBindError {
    Path(ControlSocketPathError),
    StalePathCleanup { path: PathBuf, source: io::Error },
    Socket(io::Error),
    Bind { path: PathBuf, source: io::Error },
    Listen(io::Error),
}

impl From<ControlSocketBindError> for JobsSocketBindError {
    fn from(error: ControlSocketBindError) -> Self {
        match error {
            ControlSocketBindError::Path(error) => Self::Path(error),
            ControlSocketBindError::StalePathCleanup { path, source } => {
                Self::StalePathCleanup { path, source }
            }
            ControlSocketBindError::Socket(error) => Self::Socket(error),
            ControlSocketBindError::Bind { path, source } => Self::Bind { path, source },
            ControlSocketBindError::Listen(error) => Self::Listen(error),
        }
    }
}

#[derive(Debug)]
pub enum JobsSocketAcceptError {
    Accept(io::Error),
}

#[derive(Debug)]
pub enum JobsSocketReadError {
    Recv(io::Error),
}

#[derive(Debug)]
pub enum JobsSocketWriteError {
    Send(io::Error),
}

/// One received record and what it carried (PSPU §7.4).
#[derive(Debug)]
pub struct JobsMessage {
    pub payload: Vec<u8>,
    /// The token the kernel attached, at `QUERY | IMPERSONATE | DUPLICATE`.
    pub token: Option<OwnedFd>,
    /// Descriptors passed with `SCM_RIGHTS`, in order.
    pub descriptors: Vec<OwnedFd>,
    /// The content did not fit the receive buffer; the tail is gone.
    pub truncated: bool,
    /// Ancillary data did not fit; the kernel closed what it could not deliver.
    pub control_truncated: bool,
}

#[derive(Debug)]
pub enum JobsSocketRead {
    Message(JobsMessage),
    Eof,
    WouldBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobsSocketWrite {
    Complete,
    WouldBlock,
}
