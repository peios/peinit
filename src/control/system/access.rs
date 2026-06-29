use super::model::{SystemAccessCheckError, SystemAccessCheckRequest, SystemAccessDecision};

pub trait SystemAccessChecker {
    fn check_system_access(
        &mut self,
        request: SystemAccessCheckRequest<'_>,
    ) -> Result<SystemAccessDecision, SystemAccessCheckError>;
}

#[cfg(feature = "peios-boundary")]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PeiosSystemAccessChecker;

#[cfg(feature = "peios-boundary")]
impl PeiosSystemAccessChecker {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "peios-boundary")]
impl SystemAccessChecker for PeiosSystemAccessChecker {
    fn check_system_access(
        &mut self,
        request: SystemAccessCheckRequest<'_>,
    ) -> Result<SystemAccessDecision, SystemAccessCheckError> {
        peios_check_system_access(request)
    }
}

#[cfg(feature = "peios-boundary")]
pub fn peios_control_peer_from_connected_socket(
    conn: std::os::fd::BorrowedFd<'_>,
) -> Result<super::model::ControlPeer, SystemAccessCheckError> {
    use std::os::fd::OwnedFd;

    use peios::token::Token;

    use crate::security::TokenSummary;

    let token = Token::open_peer(conn)
        .map_err(|error| SystemAccessCheckError::Boundary(error.to_string()))?;
    let identity = token
        .user()
        .map_err(|error| SystemAccessCheckError::Boundary(error.to_string()))?
        .to_string();
    let owned: OwnedFd = token.into();
    Ok(super::model::ControlPeer::owned_token_fd(
        owned,
        TokenSummary::requested_identity(identity),
    ))
}

#[cfg(feature = "peios-boundary")]
fn peios_check_system_access(
    request: SystemAccessCheckRequest<'_>,
) -> Result<SystemAccessDecision, SystemAccessCheckError> {
    use std::os::fd::BorrowedFd;

    use peios::access::AccessCheck;
    use peios::security::{AccessMask, SecurityDescriptor};

    use super::model::ControlSecurityDescriptor;

    if request.token_fd < 0 {
        return Err(SystemAccessCheckError::Boundary(format!(
            "invalid token fd {}",
            request.token_fd,
        )));
    }

    let descriptor = match request.descriptor {
        ControlSecurityDescriptor::Default => default_control_security_descriptor(),
        ControlSecurityDescriptor::RegistryBinary(bytes) => {
            SecurityDescriptor::from_validated_bytes(bytes.clone())
        }
    }
    .map_err(|error| SystemAccessCheckError::Boundary(error.to_string()))?;
    let token = unsafe { BorrowedFd::borrow_raw(request.token_fd) };
    let decision = AccessCheck::new(
        &descriptor,
        AccessMask::from_bits_retain(request.desired_access.bits()),
        control_security_generic_mapping(),
    )
    .token(token)
    .check()
    .map_err(|error| SystemAccessCheckError::Boundary(error.to_string()))?;

    Ok(SystemAccessDecision {
        allowed: decision.allowed,
        granted_access_bits: decision.granted.bits(),
    })
}

#[cfg(feature = "peios-boundary")]
fn control_security_generic_mapping() -> peios::security::GenericMapping {
    use super::model::SystemAccess;

    peios::security::GenericMapping::new(
        0,
        SystemAccess::RELOAD_CONFIG.bits(),
        SystemAccess::SHUTDOWN.bits(),
        SystemAccess::ALL.bits(),
    )
}

#[cfg(feature = "peios-boundary")]
fn default_control_security_descriptor() -> peios::Result<peios::security::SecurityDescriptor> {
    use peios::security::{AceFlags, AclBuilder, SdBuilder, Sid, WellKnown};

    use super::model::SystemAccess;

    let system = Sid::well_known(WellKnown::System);
    let administrators = Sid::well_known(WellKnown::Administrators);
    let dacl = AclBuilder::new()
        .allow(&system, SystemAccess::ALL.bits(), AceFlags::empty())
        .allow(&administrators, SystemAccess::ALL.bits(), AceFlags::empty())
        .build()?;
    SdBuilder::new()
        .owner(&system)
        .group(&administrators)
        .dacl(&dacl)
        .build()
}
