use crate::boundary::RegistryClient;
use crate::control::system::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessChecker,
};
use crate::control::wire::control_reload_config_response_line;
use crate::supervisor::Supervisor;

use super::{
    SupervisorControlCommandBodyError, SupervisorControlCommandBodyResponse,
    SupervisorControlCommandDispatch,
};

impl Supervisor {
    pub(super) fn run_control_reload_config_command<A>(
        &mut self,
        peer: &ControlPeer,
        control_security: &ControlSecurityDescriptor,
        access_checker: &mut A,
        registry: Option<&mut dyn RegistryClient>,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        A: SystemAccessChecker + ?Sized,
    {
        self.check_system_access(
            peer,
            control_security,
            access_checker,
            SystemAccess::RELOAD_CONFIG,
        )?;
        let outcome = self
            .reload_config_from_registry(
                registry.ok_or(SupervisorControlCommandBodyError::RegistryUnavailable)?,
            )
            .map_err(SupervisorControlCommandBodyError::reload_config)?;
        Ok(SupervisorControlCommandBodyResponse::Accepted {
            response_line: Some(
                control_reload_config_response_line(&outcome)
                    .map_err(SupervisorControlCommandBodyError::serialize)?,
            ),
            dispatch: Some(Box::new(SupervisorControlCommandDispatch::ReloadConfig(
                Box::new(outcome),
            ))),
            wait: None,
            access_denials: Vec::new(),
            job_access_denials: Vec::new(),
        })
    }
}
