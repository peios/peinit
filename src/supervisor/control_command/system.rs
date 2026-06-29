use crate::boundary::{Clock, ProcessController};
use crate::control::system::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessChecker,
    SystemShutdownCommandRequest,
};
use crate::control::wire::{ParsedControlRequest, control_system_ok_response_line};
use crate::supervisor::Supervisor;

use super::{
    SupervisorControlCommandBodyError, SupervisorControlCommandBodyResponse,
    SupervisorControlCommandDispatch,
};

impl Supervisor {
    pub(super) fn run_control_shutdown_command<C, P, A>(
        &mut self,
        parsed: &ParsedControlRequest,
        peer: &ControlPeer,
        control_security: &ControlSecurityDescriptor,
        access_checker: &mut A,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ?Sized,
    {
        let kind = parsed
            .shutdown_kind
            .ok_or(SupervisorControlCommandBodyError::InvalidArguments)?;
        self.check_system_access(
            peer,
            control_security,
            access_checker,
            SystemAccess::SHUTDOWN,
        )?;
        let dispatch = self
            .run_system_shutdown_command(
                SystemShutdownCommandRequest {
                    kind,
                    caller: Some(peer.summary.clone()),
                },
                controller,
                clock,
            )
            .map_err(SupervisorControlCommandBodyError::supervisor)?;
        Ok(SupervisorControlCommandBodyResponse::Accepted {
            response_line: Some(
                control_system_ok_response_line()
                    .map_err(SupervisorControlCommandBodyError::serialize)?,
            ),
            dispatch: Some(Box::new(SupervisorControlCommandDispatch::Shutdown(
                Box::new(dispatch),
            ))),
            wait: None,
            access_denials: Vec::new(),
        })
    }
}
