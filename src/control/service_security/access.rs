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
    use super::model::SERVICE_GENERIC_MAPPING as MAPPING;

    peios::security::GenericMapping::new(MAPPING.read, MAPPING.write, MAPPING.execute, MAPPING.all)
}

#[cfg(feature = "peios-boundary")]
fn default_service_security_descriptor() -> peios::Result<peios::security::SecurityDescriptor> {
    use peios::security::{AceFlags, AclBuilder, SdBuilder, Sid, WellKnown};

    use super::model::ServiceAccess;

    let system = Sid::well_known(WellKnown::System);
    let administrators = Sid::well_known(WellKnown::Administrators);
    let authenticated = Sid::well_known(WellKnown::AuthenticatedUsers);
    let dacl = AclBuilder::new()
        .allow(&system, ServiceAccess::ALL.bits(), AceFlags::empty())
        .allow(
            &administrators,
            ServiceAccess::ALL.bits(),
            AceFlags::empty(),
        )
        .allow(
            &authenticated,
            ServiceAccess::QUERY_STATUS.bits(),
            AceFlags::empty(),
        )
        .build()?;
    SdBuilder::new()
        .owner(&system)
        .group(&administrators)
        .dacl(&dacl)
        .build()
}

#[cfg(all(test, feature = "peios-boundary"))]
mod tests {
    use super::default_service_security_descriptor;
    use crate::control::service_security::DEFAULT_SERVICE_SECURITY_SDDL;

    /// The default a client is told in SDDL is the one peinit checks against.
    #[test]
    fn the_default_in_sddl_is_the_default_built() {
        let built = default_service_security_descriptor().expect("build the default");
        let said = peios::security::sddl::parse(DEFAULT_SERVICE_SECURITY_SDDL).expect("parse the SDDL");
        assert_eq!(said.as_bytes(), built.as_bytes());
    }
}
