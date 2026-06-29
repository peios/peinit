use super::model::{ServiceAccessCheckError, ServiceAccessCheckRequest, ServiceAccessDecision};

pub trait ServiceAccessChecker {
    fn check_service_access(
        &mut self,
        request: ServiceAccessCheckRequest<'_>,
    ) -> Result<ServiceAccessDecision, ServiceAccessCheckError>;
}

#[cfg(feature = "peios-boundary")]
impl ServiceAccessChecker for crate::control::system::PeiosSystemAccessChecker {
    fn check_service_access(
        &mut self,
        request: ServiceAccessCheckRequest<'_>,
    ) -> Result<ServiceAccessDecision, ServiceAccessCheckError> {
        peios_check_service_access(request)
    }
}

#[cfg(feature = "peios-boundary")]
fn peios_check_service_access(
    request: ServiceAccessCheckRequest<'_>,
) -> Result<ServiceAccessDecision, ServiceAccessCheckError> {
    use std::os::fd::BorrowedFd;

    use peios::access::AccessCheck;
    use peios::security::{AccessMask, SecurityDescriptor};

    use crate::service::ServiceSecurityDescriptor;

    if request.token_fd < 0 {
        return Err(ServiceAccessCheckError::Boundary(format!(
            "invalid token fd {}",
            request.token_fd,
        )));
    }

    let descriptor = match request.descriptor {
        ServiceSecurityDescriptor::Default => default_service_security_descriptor(),
        ServiceSecurityDescriptor::RegistryBinary(bytes) => {
            SecurityDescriptor::from_validated_bytes(bytes.clone())
        }
    }
    .map_err(|error| ServiceAccessCheckError::Boundary(error.to_string()))?;
    let token = unsafe { BorrowedFd::borrow_raw(request.token_fd) };
    let decision = AccessCheck::new(
        &descriptor,
        AccessMask::from_bits_retain(request.desired_access.bits()),
        service_security_generic_mapping(),
    )
    .token(token)
    .check()
    .map_err(|error| ServiceAccessCheckError::Boundary(error.to_string()))?;

    Ok(ServiceAccessDecision {
        allowed: decision.allowed,
        granted_access_bits: decision.granted.bits(),
    })
}

#[cfg(feature = "peios-boundary")]
fn service_security_generic_mapping() -> peios::security::GenericMapping {
    use super::model::ServiceAccess;

    let write_execute = ServiceAccess::START
        .union(ServiceAccess::STOP)
        .union(ServiceAccess::INTERROGATE);
    peios::security::GenericMapping::new(
        ServiceAccess::QUERY_STATUS.bits(),
        write_execute.bits(),
        write_execute.bits(),
        ServiceAccess::ALL.bits(),
    )
}

#[cfg(feature = "peios-boundary")]
fn default_service_security_descriptor() -> peios::Result<peios::security::SecurityDescriptor> {
    use peios::security::{AceFlags, AclBuilder, SdBuilder, Sid, WellKnown};

    use super::model::ServiceAccess;

    let system = Sid::well_known(WellKnown::System);
    let administrators = Sid::well_known(WellKnown::Administrators);
    let admin_access = ServiceAccess::QUERY_STATUS.union(ServiceAccess::STOP);
    let dacl = AclBuilder::new()
        .allow(&system, ServiceAccess::ALL.bits(), AceFlags::empty())
        .allow(&administrators, admin_access.bits(), AceFlags::empty())
        .build()?;
    SdBuilder::new()
        .owner(&system)
        .group(&administrators)
        .dacl(&dacl)
        .build()
}
