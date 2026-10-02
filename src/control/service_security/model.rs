use crate::control::lifecycle::LifecycleCommand;
use crate::security::TokenSummary;
use crate::service::ServiceSecurityDescriptor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceAccess(u32);

impl ServiceAccess {
    pub const QUERY_STATUS: Self = Self(0x0001);
    pub const START: Self = Self(0x0002);
    pub const STOP: Self = Self(0x0004);
    pub const INTERROGATE: Self = Self(0x0008);
    pub const ALL: Self =
        Self(Self::QUERY_STATUS.0 | Self::START.0 | Self::STOP.0 | Self::INTERROGATE.0);

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The right a lifecycle command needs (§4.6). Restart asks for start and
    /// stop as one mask, and reset needs stop, because clearing a Failed or
    /// Abandoned state is the tail of stopping something.
    pub const fn for_command(command: LifecycleCommand) -> Self {
        match command {
            LifecycleCommand::Start => Self::START,
            LifecycleCommand::Stop => Self::STOP,
            LifecycleCommand::Restart => Self::START.union(Self::STOP),
            LifecycleCommand::Reload => Self::INTERROGATE,
            LifecycleCommand::Reset => Self::STOP,
        }
    }
}

/// What the generic rights stand for on a service, as AccessCheck is told
/// them (§4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceGenericMapping {
    pub read: u32,
    pub write: u32,
    pub execute: u32,
    pub all: u32,
}

pub const SERVICE_GENERIC_MAPPING: ServiceGenericMapping = ServiceGenericMapping {
    read: ServiceAccess::QUERY_STATUS.bits(),
    write: ServiceAccess::START
        .union(ServiceAccess::STOP)
        .union(ServiceAccess::INTERROGATE)
        .bits(),
    execute: ServiceAccess::START
        .union(ServiceAccess::STOP)
        .union(ServiceAccess::INTERROGATE)
        .bits(),
    all: ServiceAccess::ALL.bits(),
};

/// The descriptor a service takes when neither its definition nor the
/// Services key carries one (§4.6), in SDDL: SYSTEM and Administrators may
/// do everything, and everyone authenticated may see its state. The boundary
/// builds the same descriptor, and a test holds the two to the same bytes.
pub const DEFAULT_SERVICE_SECURITY_SDDL: &str =
    "O:SYG:BAD:(A;;0xf;;;SY)(A;;0xf;;;BA)(A;;0x1;;;AU)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceAccessCheckRequest<'a> {
    pub token_fd: i32,
    pub service: &'a str,
    pub descriptor: &'a ServiceSecurityDescriptor,
    pub desired_access: ServiceAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceAccessDecision {
    pub allowed: bool,
    pub granted_access_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceAccessDenied {
    pub caller: TokenSummary,
    pub service: String,
    pub desired_access: ServiceAccess,
    pub granted_access_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceAccessCheckError {
    Boundary(String),
}
