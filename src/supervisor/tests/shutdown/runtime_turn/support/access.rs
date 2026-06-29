use crate::control::service_security::{
    ServiceAccess, ServiceAccessCheckError, ServiceAccessCheckRequest, ServiceAccessChecker,
    ServiceAccessDecision,
};
use crate::control::system::{
    SystemAccess, SystemAccessCheckError, SystemAccessCheckRequest, SystemAccessChecker,
    SystemAccessDecision,
};

#[derive(Debug, Default)]
pub(crate) struct AllowAccessChecker {
    calls: Vec<SystemAccessCheckRequestSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SystemAccessCheckRequestSummary {
    token_fd: i32,
    desired_access: SystemAccess,
}

impl SystemAccessChecker for AllowAccessChecker {
    fn check_system_access(
        &mut self,
        request: SystemAccessCheckRequest<'_>,
    ) -> Result<SystemAccessDecision, SystemAccessCheckError> {
        self.calls.push(SystemAccessCheckRequestSummary {
            token_fd: request.token_fd,
            desired_access: request.desired_access,
        });
        Ok(SystemAccessDecision {
            allowed: true,
            granted_access_bits: SystemAccess::ALL.bits(),
        })
    }
}

impl ServiceAccessChecker for AllowAccessChecker {
    fn check_service_access(
        &mut self,
        _request: ServiceAccessCheckRequest<'_>,
    ) -> Result<ServiceAccessDecision, ServiceAccessCheckError> {
        Ok(ServiceAccessDecision {
            allowed: true,
            granted_access_bits: ServiceAccess::ALL.bits(),
        })
    }
}
