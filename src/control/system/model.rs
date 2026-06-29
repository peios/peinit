use std::os::fd::{AsRawFd, OwnedFd};

use crate::security::TokenSummary;
use crate::shutdown::ShutdownKind;

#[derive(Debug)]
pub struct ControlPeer {
    pub token: ControlPeerToken,
    pub summary: TokenSummary,
}

impl ControlPeer {
    pub fn borrowed_token_fd(token_fd: i32, summary: TokenSummary) -> Self {
        Self {
            token: ControlPeerToken::BorrowedRawFd(token_fd),
            summary,
        }
    }

    pub fn owned_token_fd(token_fd: OwnedFd, summary: TokenSummary) -> Self {
        Self {
            token: ControlPeerToken::OwnedFd(token_fd),
            summary,
        }
    }

    pub fn token_fd(&self) -> i32 {
        match &self.token {
            ControlPeerToken::BorrowedRawFd(fd) => *fd,
            ControlPeerToken::OwnedFd(fd) => fd.as_raw_fd(),
        }
    }
}

#[derive(Debug)]
pub enum ControlPeerToken {
    BorrowedRawFd(i32),
    OwnedFd(OwnedFd),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemAccess(u32);

impl SystemAccess {
    pub const SHUTDOWN: Self = Self(0x0001);
    pub const RELOAD_CONFIG: Self = Self(0x0002);
    pub const ALL: Self = Self(Self::SHUTDOWN.0 | Self::RELOAD_CONFIG.0);

    pub const fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ControlSecurityDescriptor {
    #[default]
    Default,
    RegistryBinary(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemAccessCheckRequest<'a> {
    pub token_fd: i32,
    pub descriptor: &'a ControlSecurityDescriptor,
    pub desired_access: SystemAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemAccessDecision {
    pub allowed: bool,
    pub granted_access_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemAccessDenied {
    pub caller: TokenSummary,
    pub desired_access: SystemAccess,
    pub granted_access_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemAccessCheckError {
    Boundary(String),
}

/// Parsed and already-authorized system shutdown command.
///
/// Control-socket peer authentication and KACS AccessCheck are separate
/// boundary work; this model is the pure command admitted after those checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemShutdownCommandRequest {
    pub kind: ShutdownKind,
    pub caller: Option<TokenSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemShutdownCommandOutcome {
    pub kind: ShutdownKind,
    pub caller: Option<TokenSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemShutdownCommandAdmissionError {
    InvalidCommand,
    InvalidArguments,
}
