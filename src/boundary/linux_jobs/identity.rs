use std::os::fd::{BorrowedFd, IntoRawFd, OwnedFd};

use peios::token::{ImpersonationLevel, Token, TokenAccess, TokenType};

use crate::boundary::{
    BoundaryError, JobIdentityError, JobIdentityProvider, JobIdentitySource, PreparedJobIdentity,
    TokenHandle,
};
use crate::job::JobRecord;

use crate::boundary::linux_launch::summarize_token_for_identity;

/// Prepares a job identity by exactly the two routes PSPU §7.5 allows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinuxJobIdentityProvider;

impl LinuxJobIdentityProvider {
    pub fn new() -> Self {
        Self
    }
}

impl JobIdentityProvider for LinuxJobIdentityProvider {
    fn prepare_job_identity(
        &mut self,
        source: JobIdentitySource,
    ) -> Result<PreparedJobIdentity, JobIdentityError> {
        let source = match source {
            // The kernel gated the attach with the submitter as installer and
            // recorded the clamped level on the token; the fd carries
            // QUERY | IMPERSONATE | DUPLICATE.
            JobIdentitySource::AttachedToken { token } => Token::from(token),
            // The connecting process's own primary, through the kernel's
            // handle on that process. SYSTEM holds TOKEN_ALL_ACCESS on every
            // token's default descriptor, so DUPLICATE is granted.
            JobIdentitySource::PeerPrimary { pidfd } => {
                let pidfd = unsafe { BorrowedFd::borrow_raw(pidfd) };
                Token::open_process(pidfd, TokenAccess::QUERY | TokenAccess::DUPLICATE).map_err(
                    |error| {
                        JobIdentityError::Boundary(format!(
                            "open peer primary token failed: {error}"
                        ))
                    },
                )?
            }
        };
        let level = source.impersonation_level().map_err(|error| {
            JobIdentityError::Boundary(format!("query token impersonation level failed: {error}"))
        })?;
        if level < ImpersonationLevel::Impersonation {
            return Err(JobIdentityError::BadToken(format!(
                "token level {level:?} is below Impersonation and cannot run a process"
            )));
        }
        // The ratchet carries the source's level onto the primary (Kernel TRM
        // §3.5.1); asking for exactly that level is what the kernel permits.
        let primary = source
            .duplicate(TokenAccess::ALL_ACCESS, TokenType::Primary, level)
            .map_err(|error| {
                JobIdentityError::BadToken(format!("duplicate token to primary failed: {error}"))
            })?;
        let user_sid = primary
            .user()
            .map_err(|error| {
                JobIdentityError::Boundary(format!("query prepared token user failed: {error}"))
            })?
            .to_string();
        let logon_session = primary
            .auth_id()
            .map_err(|error| {
                JobIdentityError::Boundary(format!("query prepared token session failed: {error}"))
            })?
            .0;
        let summary = summarize_token_for_identity(&user_sid, &primary)
            .map_err(|error| JobIdentityError::Boundary(format!("{error:?}")))?;
        let token_fd: OwnedFd = primary.into();
        Ok(PreparedJobIdentity {
            token_fd: token_fd.into_raw_fd(),
            user_sid,
            logon_session,
            summary,
        })
    }
}

/// The launch-side half: turn a prepared token back into the handle the
/// child path installs.
pub(in crate::boundary) fn materialize_linux_prepared_token(
    job: &JobRecord,
    prepared_token_fd: i32,
) -> Result<TokenHandle, BoundaryError> {
    if prepared_token_fd < 0 {
        return Err(BoundaryError::Token(format!(
            "job {} has no prepared token",
            job.id
        )));
    }
    let token = unsafe { BorrowedFd::borrow_raw(prepared_token_fd) };
    let token = Token::from(
        token
            .try_clone_to_owned()
            .map_err(|error| BoundaryError::Token(format!("duplicate prepared token fd: {error}")))?,
    );
    let summary = summarize_token_for_identity(&job.resolved_identity, &token)?;
    // The launch owns the fd it is handed and closes it after the clone;
    // the original prepared fd is closed by the caller once the launch has
    // taken its copy.
    Ok(TokenHandle {
        fd: OwnedFd::from(token).into_raw_fd(),
        identity: job.resolved_identity.clone(),
        summary,
    })
}
