//! The boundary a submitted job's identity crosses (PSPU §7.5).
//!
//! The core decides *which* of the two identity paths a submission takes; the
//! boundary performs the token operations — opening the peer's primary by its
//! process handle, or taking the token the kernel attached — and duplicates
//! the result to the primary token the job's process is installed with.

use std::os::fd::OwnedFd;

use crate::security::TokenSummary;

/// Where a job identity comes from. Exactly one of the two, by design: a
/// primary token passed as a plain descriptor is not accepted.
#[derive(Debug)]
pub enum JobIdentitySource {
    /// The kernel attached a token to the `submit` message: the submitter
    /// could act as this identity, at the level recorded on the token.
    AttachedToken { token: OwnedFd },
    /// Nothing was attached: the job runs as the connecting *process's* own
    /// primary token, opened through the kernel's handle on that process.
    PeerPrimary { pidfd: i32 },
}

/// A primary token ready to be installed on a job, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedJobIdentity {
    /// The duplicated primary token. Raw and owned by the caller, as every
    /// descriptor handed into the job model is; the launch closes it.
    pub token_fd: i32,
    pub user_sid: String,
    pub logon_session: u64,
    pub summary: TokenSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobIdentityError {
    /// The token cannot be a job identity — below Impersonation level, or
    /// not duplicable. Answered `BAD_TOKEN`.
    BadToken(String),
    /// A failure that is not the token's fault.
    Boundary(String),
}

pub trait JobIdentityProvider {
    fn prepare_job_identity(
        &mut self,
        source: JobIdentitySource,
    ) -> Result<PreparedJobIdentity, JobIdentityError>;
}

impl<T: JobIdentityProvider + ?Sized> JobIdentityProvider for &mut T {
    fn prepare_job_identity(
        &mut self,
        source: JobIdentitySource,
    ) -> Result<PreparedJobIdentity, JobIdentityError> {
        (**self).prepare_job_identity(source)
    }
}
