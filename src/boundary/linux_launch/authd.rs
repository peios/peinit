//! Asking the authority for a service's token.
//!
//! peinit mints SYSTEM tokens itself, because it must: the authority is a
//! service, and something has to start it. Every other identity comes from
//! here.
//!
//! The reason for the split is not that peinit lacks the privilege — it plainly
//! has it, three files over. It is that the privilege set and integrity level
//! an identity carries are **policy**, and policy is the authority's. A second
//! component deciding it in parallel is a disagreement waiting to happen, and
//! this crate has already shipped one: its hand-written privilege bit table had
//! four wrong entries, silently stripping privileges a service had asked to
//! keep.
//!
//! # This request must come from PID 1 itself
//!
//! The authority authorises a `ServiceAttest` on two facts, and the second is
//! that the peer is PID 1 (PGSS Logon §2.19). That makes it a **standing
//! constraint on this crate**: the connection must be opened by peinit's own
//! process and never by a forked helper.
//!
//! Nothing enforces it here, and if it is ever broken the symptom is every
//! non-SYSTEM service failing to start at boot with an authorisation error —
//! which reads like an authority problem rather than like a refactor. Keep the
//! call on this side of any fork.
//!
//! # Fail closed
//!
//! An authority that cannot be reached fails the service start. The stub this
//! replaced returned a SYSTEM token instead, which meant a service declaring
//! `Identity = LocalService` ran with peinit's entire privilege set and
//! `svctl status` reported it as `LocalService` — a wrong answer that looked
//! like a right one. A service that does not start is visible; a service
//! running as the wrong principal is not.

use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use libauthd::transport::{recv_message_with_fd, send_message};
use libauthd::wire::{
    self, MSG_ACCESS_DENIED, MSG_ACCESS_GRANTED, ServiceAttest, decode_access_denied,
    decode_header, encode_service_attest,
};
use peios::token::Token;

use crate::boundary::BoundaryError;

/// Where the authority listens. PGSS Logon §2.5.
const LOGON_SOCKET: &str = "/run/logon.sock";

/// How long to wait on the authority before failing the service start.
///
/// Finite because a wedged authority must not wedge the boot: peinit is PID 1,
/// and a blocked service start blocks everything ordered behind it. Generous
/// because minting a token is a kernel round trip and a policy read, and a
/// machine under load at boot is exactly when this runs.
const AUTHD_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AuthdTokenRequest {
    pub identity: String,
    pub service: String,
}

pub(super) trait AuthdTokenClient {
    fn request_service_token(&mut self, request: AuthdTokenRequest)
    -> Result<Token, BoundaryError>;
}

/// Speaks PGSS Logon §2.19 to the authority on `/run/logon.sock`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct SocketAuthdTokenClient;

impl AuthdTokenClient for SocketAuthdTokenClient {
    fn request_service_token(
        &mut self,
        request: AuthdTokenRequest,
    ) -> Result<Token, BoundaryError> {
        attest(LOGON_SOCKET, &request).map_err(|error| {
            BoundaryError::Token(format!(
                "authority would not issue a token for {} as {}: {error}",
                request.service, request.identity
            ))
        })
    }
}

/// One request, one reply. A `ServiceAttest` is not a conversation: it carries
/// no credential, so there is nothing to exchange and no round to loop over.
///
/// The blocking calls are retried on `EINTR`. This runs in PID 1 with a
/// socket timeout set, and a timed `recvmsg` interrupted by a ptrace stop
/// returns `EINTR` even with no handler installed — so a debugger attaching
/// to peinit mid-launch used to fail the service with "token materialization
/// failed: Interrupted system call" (PEI-1085). The send needs no wrapper:
/// `write_all` already retries an interrupted write. `recvmsg` with
/// `MSG_WAITALL` returns a short count rather than `EINTR` once any byte has
/// arrived, so retrying it never re-reads a header.
fn attest(path: &str, request: &AuthdTokenRequest) -> io::Result<Token> {
    let socket = retry_interrupted(|| UnixStream::connect(path))?;
    socket.set_read_timeout(Some(AUTHD_TIMEOUT))?;
    socket.set_write_timeout(Some(AUTHD_TIMEOUT))?;

    let message = encode_service_attest(&ServiceAttest {
        identity: request.identity.clone(),
        service: request.service.clone(),
    })
    .map_err(|error| io::Error::other(format!("could not encode the request: {error:?}")))?;
    send_message(&socket, &message)?;

    let (reply, descriptor) = retry_interrupted(|| recv_message_with_fd(&wire::FRAMING, &socket))?;
    let (message_type, _) = decode_header(reply.expose())
        .map_err(|error| io::Error::other(format!("malformed reply: {error:?}")))?;

    match message_type {
        MSG_ACCESS_GRANTED => {
            // A grant without a descriptor is a protocol violation, and the one
            // failure that must never be treated as success: proceeding here
            // would launch the service with whatever token the process already
            // had, which is peinit's.
            let descriptor: OwnedFd = descriptor.ok_or_else(|| {
                io::Error::other("the authority granted a token but sent no descriptor")
            })?;
            Ok(Token::from(descriptor))
        }
        MSG_ACCESS_DENIED => {
            let denied = decode_access_denied(reply.expose())
                .map_err(|error| io::Error::other(format!("malformed denial: {error:?}")))?;
            Err(io::Error::other(format!(
                "{:?}: {}",
                denied.denial, denied.reason
            )))
        }
        other => Err(io::Error::other(format!(
            "unexpected reply type {other:#06x}"
        ))),
    }
}

/// Run `operation` again for as long as it fails with `EINTR`.
///
/// Unbounded, like the crate's other `EINTR` loops: each retry costs one
/// interruption, and nothing interrupts PID 1 in a loop.
fn retry_interrupted<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    loop {
        match operation() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PEI-1085. A ptrace stop while PID 1 waits on the authority must not
    /// fail the launch.
    #[test]
    fn an_interrupted_call_is_retried_and_a_real_failure_is_not() {
        let mut attempts = 0;
        let value = retry_interrupted(|| {
            attempts += 1;
            if attempts < 3 {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            } else {
                Ok(attempts)
            }
        })
        .expect("retried past the interruptions");
        assert_eq!(value, 3);

        let mut attempts = 0;
        let error = retry_interrupted(|| {
            attempts += 1;
            Err::<(), _>(io::Error::from(io::ErrorKind::ConnectionRefused))
        })
        .expect_err("a real failure");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
        assert_eq!(attempts, 1);
    }

    /// The failure that matters. A service start that cannot reach the
    /// authority must fail, not fall back — the whole point of moving minting
    /// out of this crate is lost if it silently mints anyway.
    #[test]
    fn an_unreachable_authority_fails_the_request() {
        let error = attest(
            "/nonexistent/logon.sock",
            &AuthdTokenRequest {
                identity: "LocalService".to_string(),
                service: "resolvd".to_string(),
            },
        )
        .expect_err("no authority is listening there");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
