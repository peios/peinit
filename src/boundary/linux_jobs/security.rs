use std::os::fd::BorrowedFd;
use std::str::FromStr;

use peios::access::AccessCheck;
use peios::security::{
    AccessMask, AceFlags, AclBuilder, GenericMapping, SdBuilder, SecurityDescriptor, Sid,
    WellKnown, sddl,
};

use crate::control::system::PeiosSystemAccessChecker;
use crate::submitted::{
    JobAccess, JobAccessCheckError, JobAccessCheckRequest, JobAccessChecker, JobAccessDecision,
    JobDescriptorError, JobDescriptorFactory, JobSecurityDescriptor,
};

impl JobDescriptorFactory for PeiosSystemAccessChecker {
    /// The default of PSPU §7.8: owner and group the submitter, full control
    /// to the submitter, SYSTEM and Administrators, nothing to anyone else —
    /// the job identity included.
    fn default_job_descriptor(
        &mut self,
        submitter_sid: &str,
    ) -> Result<JobSecurityDescriptor, JobDescriptorError> {
        let submitter = Sid::from_str(submitter_sid).map_err(|error| {
            JobDescriptorError::Boundary(format!("submitter SID {submitter_sid:?}: {error}"))
        })?;
        let system = Sid::well_known(WellKnown::System);
        let administrators = Sid::well_known(WellKnown::Administrators);
        let dacl = AclBuilder::new()
            .allow(&submitter, JobAccess::ALL.bits(), AceFlags::empty())
            .allow(&system, JobAccess::ALL.bits(), AceFlags::empty())
            .allow(&administrators, JobAccess::ALL.bits(), AceFlags::empty())
            .build()
            .map_err(|error| JobDescriptorError::Boundary(error.to_string()))?;
        let descriptor = SdBuilder::new()
            .owner(&submitter)
            .group(&submitter)
            .dacl(&dacl)
            .build()
            .map_err(|error| JobDescriptorError::Boundary(error.to_string()))?;
        Ok(JobSecurityDescriptor {
            bytes: descriptor.as_bytes().to_vec(),
        })
    }

    /// A submitter-supplied descriptor, used as given — with an owner and a
    /// DACL, or refused.
    fn job_descriptor_from_sddl(
        &mut self,
        text: &str,
    ) -> Result<JobSecurityDescriptor, JobDescriptorError> {
        let descriptor =
            sddl::parse(text).map_err(|error| JobDescriptorError::Invalid(error.to_string()))?;
        let view = descriptor
            .view()
            .map_err(|error| JobDescriptorError::Invalid(error.to_string()))?;
        if view.owner().is_none() {
            return Err(JobDescriptorError::Invalid(
                "descriptor has no owner".to_string(),
            ));
        }
        if view.dacl().is_none() {
            return Err(JobDescriptorError::Invalid(
                "descriptor has no DACL".to_string(),
            ));
        }
        Ok(JobSecurityDescriptor {
            bytes: descriptor.as_bytes().to_vec(),
        })
    }
}

impl JobAccessChecker for PeiosSystemAccessChecker {
    fn check_job_access(
        &mut self,
        request: JobAccessCheckRequest<'_>,
    ) -> Result<JobAccessDecision, JobAccessCheckError> {
        if request.token_fd < 0 {
            return Err(JobAccessCheckError::Boundary(format!(
                "invalid token fd {}",
                request.token_fd
            )));
        }
        let descriptor = SecurityDescriptor::from_validated_bytes(request.descriptor.bytes.clone())
            .map_err(|error| JobAccessCheckError::Boundary(error.to_string()))?;
        let token = unsafe { BorrowedFd::borrow_raw(request.token_fd) };
        let decision = AccessCheck::new(
            &descriptor,
            AccessMask::from_bits_retain(request.desired_access.bits()),
            job_generic_mapping(),
        )
        .token(token)
        .check()
        .map_err(|error| JobAccessCheckError::Boundary(error.to_string()))?;
        Ok(JobAccessDecision {
            allowed: decision.allowed,
            granted_access_bits: decision.granted.bits(),
        })
    }
}

/// PSPU §7.8: read → query; write and execute → stop and signal.
fn job_generic_mapping() -> GenericMapping {
    let write_execute = JobAccess::STOP.union(JobAccess::SIGNAL);
    GenericMapping::new(
        JobAccess::QUERY.bits(),
        write_execute.bits(),
        write_execute.bits(),
        JobAccess::ALL.bits(),
    )
}
