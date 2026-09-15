use crate::control::query::ServiceListItem;
use crate::control::service_security::{
    ServiceAccess, ServiceAccessCheckRequest, ServiceAccessChecker, ServiceAccessDecision,
    ServiceAccessDenied,
};
use crate::control::system::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessCheckRequest,
    SystemAccessChecker, SystemAccessDenied,
};
use crate::control::wire::ControlCommand;

use super::SupervisorControlCommandBodyError;
use crate::supervisor::Supervisor;

impl Supervisor {
    pub(super) fn check_system_access<A>(
        &self,
        peer: &ControlPeer,
        control_security: &ControlSecurityDescriptor,
        access_checker: &mut A,
        desired_access: SystemAccess,
    ) -> Result<(), SupervisorControlCommandBodyError>
    where
        A: SystemAccessChecker + ?Sized,
    {
        let decision = access_checker
            .check_system_access(SystemAccessCheckRequest {
                token_fd: peer.token_fd(),
                descriptor: control_security,
                desired_access,
            })
            .map_err(SupervisorControlCommandBodyError::SystemAuthorization)?;
        if decision.allowed {
            Ok(())
        } else {
            Err(SupervisorControlCommandBodyError::SystemAccessDenied(
                Box::new(SystemAccessDenied {
                    caller: peer.summary.clone(),
                    desired_access,
                    granted_access_bits: decision.granted_access_bits,
                }),
            ))
        }
    }

    pub(super) fn check_service_command_access<A>(
        &self,
        peer: &ControlPeer,
        access_checker: &mut A,
        service: &str,
        command: ControlCommand,
    ) -> Result<(), SupervisorControlCommandBodyError>
    where
        A: ServiceAccessChecker + ?Sized,
    {
        if self.definition_removed_is_unknown_for_command(service, command)? {
            return Err(SupervisorControlCommandBodyError::UnknownService {
                service: service.to_string(),
            });
        }
        self.check_service_access(
            peer,
            access_checker,
            service,
            service_access_for_command(command)?,
        )
    }

    pub(super) fn check_service_access<A>(
        &self,
        peer: &ControlPeer,
        access_checker: &mut A,
        service: &str,
        desired_access: ServiceAccess,
    ) -> Result<(), SupervisorControlCommandBodyError>
    where
        A: ServiceAccessChecker + ?Sized,
    {
        let entry = self.services.get(service).ok_or_else(|| {
            SupervisorControlCommandBodyError::UnknownService {
                service: service.to_string(),
            }
        })?;
        self.check_service_access_with_descriptor(
            peer,
            access_checker,
            service,
            &entry.definition.service_security,
            desired_access,
        )
    }

    /// The service access check against a descriptor the caller already
    /// holds — for a target that is no longer in the table, where the
    /// descriptor is the one recorded on the operation (PEI-1076).
    pub(super) fn check_service_access_with_descriptor<A>(
        &self,
        peer: &ControlPeer,
        access_checker: &mut A,
        service: &str,
        descriptor: &crate::service::ServiceSecurityDescriptor,
        desired_access: ServiceAccess,
    ) -> Result<(), SupervisorControlCommandBodyError>
    where
        A: ServiceAccessChecker + ?Sized,
    {
        let decision = access_checker
            .check_service_access(ServiceAccessCheckRequest {
                token_fd: peer.token_fd(),
                service,
                descriptor,
                desired_access,
            })
            .map_err(SupervisorControlCommandBodyError::ServiceAuthorization)?;
        if decision.allowed {
            Ok(())
        } else {
            Err(SupervisorControlCommandBodyError::ServiceAccessDenied(
                Box::new(ServiceAccessDenied {
                    caller: peer.summary.clone(),
                    service: service.to_string(),
                    desired_access,
                    granted_access_bits: decision.granted_access_bits,
                }),
            ))
        }
    }

    pub(super) fn list_authorized_services<A>(
        &self,
        peer: &ControlPeer,
        access_checker: &mut A,
    ) -> Result<(Vec<ServiceListItem>, Vec<ServiceAccessDenied>), SupervisorControlCommandBodyError>
    where
        A: ServiceAccessChecker + ?Sized,
    {
        let mut services = Vec::new();
        let mut denied = Vec::new();
        for item in self.list_services() {
            let decision = self.service_access_decision(
                peer,
                access_checker,
                &item.service,
                ServiceAccess::QUERY_STATUS,
            )?;
            if decision.allowed {
                services.push(item);
            } else {
                denied.push(ServiceAccessDenied {
                    caller: peer.summary.clone(),
                    service: item.service,
                    desired_access: ServiceAccess::QUERY_STATUS,
                    granted_access_bits: decision.granted_access_bits,
                });
            }
        }
        Ok((services, denied))
    }

    fn definition_removed_is_unknown_for_command(
        &self,
        service: &str,
        command: ControlCommand,
    ) -> Result<bool, SupervisorControlCommandBodyError> {
        let entry = self.services.get(service).ok_or_else(|| {
            SupervisorControlCommandBodyError::UnknownService {
                service: service.to_string(),
            }
        })?;
        Ok(entry.definition_removed
            && matches!(
                command,
                ControlCommand::Start | ControlCommand::Restart | ControlCommand::Reload
            ))
    }

    fn service_access_decision<A>(
        &self,
        peer: &ControlPeer,
        access_checker: &mut A,
        service: &str,
        desired_access: ServiceAccess,
    ) -> Result<ServiceAccessDecision, SupervisorControlCommandBodyError>
    where
        A: ServiceAccessChecker + ?Sized,
    {
        let entry = self.services.get(service).ok_or_else(|| {
            SupervisorControlCommandBodyError::UnknownService {
                service: service.to_string(),
            }
        })?;
        access_checker
            .check_service_access(ServiceAccessCheckRequest {
                token_fd: peer.token_fd(),
                service,
                descriptor: &entry.definition.service_security,
                desired_access,
            })
            .map_err(SupervisorControlCommandBodyError::ServiceAuthorization)
    }
}

fn service_access_for_command(
    command: ControlCommand,
) -> Result<ServiceAccess, SupervisorControlCommandBodyError> {
    match command {
        ControlCommand::Start => Ok(ServiceAccess::START),
        ControlCommand::Stop => Ok(ServiceAccess::STOP),
        ControlCommand::Restart => Ok(ServiceAccess::START.union(ServiceAccess::STOP)),
        ControlCommand::Reload => Ok(ServiceAccess::INTERROGATE),
        ControlCommand::Reset => Ok(ServiceAccess::STOP),
        ControlCommand::Status => Ok(ServiceAccess::QUERY_STATUS),
        _ => Err(SupervisorControlCommandBodyError::InvalidArguments),
    }
}
