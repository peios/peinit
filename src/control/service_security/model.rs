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
}

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
