//! Per-job access control (PSPU §7.8): the rights, the descriptor every
//! submitted job carries, and the two boundary traits that mint and evaluate
//! descriptors without the core knowing their bytes.

use crate::security::TokenSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobAccess(u32);

impl JobAccess {
    pub const QUERY: Self = Self(0x0001);
    pub const STOP: Self = Self(0x0002);
    pub const SIGNAL: Self = Self(0x0004);
    pub const ALL: Self = Self(Self::QUERY.0 | Self::STOP.0 | Self::SIGNAL.0);

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn label(self) -> &'static str {
        match self.0 {
            0x0001 => "JOB_QUERY",
            0x0002 => "JOB_STOP",
            0x0004 => "JOB_SIGNAL",
            0x0007 => "JOB_ALL_ACCESS",
            _ => "JOB_ACCESS",
        }
    }
}

/// A job's Security Descriptor, as validated binary bytes. The core never
/// parses it; it is handed to the boundary for every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSecurityDescriptor {
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobDescriptorError {
    /// The submitter's SDDL did not parse, or parsed to a descriptor
    /// without an owner and a DACL.
    Invalid(String),
    Boundary(String),
}

/// Mints job descriptors: the default of §7.8 for a submitter, or one the
/// submitter supplied in SDDL.
pub trait JobDescriptorFactory {
    fn default_job_descriptor(
        &mut self,
        submitter_sid: &str,
    ) -> Result<JobSecurityDescriptor, JobDescriptorError>;

    fn job_descriptor_from_sddl(
        &mut self,
        sddl: &str,
    ) -> Result<JobSecurityDescriptor, JobDescriptorError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobAccessCheckRequest<'a> {
    pub token_fd: i32,
    pub descriptor: &'a JobSecurityDescriptor,
    pub desired_access: JobAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobAccessDecision {
    pub allowed: bool,
    pub granted_access_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobAccessDenied {
    pub caller: TokenSummary,
    pub job_id: crate::ids::JobId,
    pub desired_access: JobAccess,
    pub granted_access_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobAccessCheckError {
    Boundary(String),
}

pub trait JobAccessChecker {
    fn check_job_access(
        &mut self,
        request: JobAccessCheckRequest<'_>,
    ) -> Result<JobAccessDecision, JobAccessCheckError>;
}

impl<T: JobDescriptorFactory + ?Sized> JobDescriptorFactory for &mut T {
    fn default_job_descriptor(
        &mut self,
        submitter_sid: &str,
    ) -> Result<JobSecurityDescriptor, JobDescriptorError> {
        (**self).default_job_descriptor(submitter_sid)
    }

    fn job_descriptor_from_sddl(
        &mut self,
        sddl: &str,
    ) -> Result<JobSecurityDescriptor, JobDescriptorError> {
        (**self).job_descriptor_from_sddl(sddl)
    }
}

impl<T: JobAccessChecker + ?Sized> JobAccessChecker for &mut T {
    fn check_job_access(
        &mut self,
        request: JobAccessCheckRequest<'_>,
    ) -> Result<JobAccessDecision, JobAccessCheckError> {
        (**self).check_job_access(request)
    }
}
