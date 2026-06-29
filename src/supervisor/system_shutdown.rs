mod model;

pub use model::{
    SupervisorSystemShutdownControlBodyError, SupervisorSystemShutdownControlBodyResponse,
    system_shutdown_control_response_line,
};

use crate::boundary::{Clock, ProcessController};
use crate::control::system::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessCheckRequest,
    SystemAccessChecker, SystemAccessDenied, SystemShutdownCommandOutcome,
    SystemShutdownCommandRequest, admit_system_shutdown_command,
};
use crate::control::wire::parse_control_request;
use crate::security::TokenSummary;

use super::dispatch::SupervisorSystemShutdownDispatch;
use super::state::{Supervisor, SupervisorError};

impl Supervisor {
    pub fn run_system_shutdown_command<C, P>(
        &mut self,
        request: SystemShutdownCommandRequest,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<SupervisorSystemShutdownDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
    {
        let observed_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let shutdown = self.begin_shutdown(request.kind, controller, observed_at_ns)?;
        Ok(SupervisorSystemShutdownDispatch {
            command: SystemShutdownCommandOutcome {
                kind: request.kind,
                caller: request.caller,
            },
            shutdown,
        })
    }

    pub fn run_authorized_shutdown_control_body<C, P>(
        &mut self,
        body: &[u8],
        caller: Option<TokenSummary>,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<SupervisorSystemShutdownDispatch, SupervisorSystemShutdownControlBodyError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
    {
        let parsed =
            parse_control_request(body).map_err(SupervisorSystemShutdownControlBodyError::Parse)?;
        let request = admit_system_shutdown_command(&parsed, caller)
            .map_err(SupervisorSystemShutdownControlBodyError::Admission)?;
        self.run_system_shutdown_command(request, controller, clock)
            .map_err(SupervisorSystemShutdownControlBodyError::supervisor)
    }

    pub fn run_checked_shutdown_control_body<C, P, A>(
        &mut self,
        body: &[u8],
        peer: &ControlPeer,
        control_security: &ControlSecurityDescriptor,
        access_checker: &mut A,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<SupervisorSystemShutdownDispatch, SupervisorSystemShutdownControlBodyError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ?Sized,
    {
        let parsed =
            parse_control_request(body).map_err(SupervisorSystemShutdownControlBodyError::Parse)?;
        let request = admit_system_shutdown_command(&parsed, Some(peer.summary.clone()))
            .map_err(SupervisorSystemShutdownControlBodyError::Admission)?;
        let desired_access = SystemAccess::SHUTDOWN;
        let decision = access_checker
            .check_system_access(SystemAccessCheckRequest {
                token_fd: peer.token_fd(),
                descriptor: control_security,
                desired_access,
            })
            .map_err(SupervisorSystemShutdownControlBodyError::Authorization)?;

        if !decision.allowed {
            return Err(SupervisorSystemShutdownControlBodyError::AccessDenied(
                Box::new(SystemAccessDenied {
                    caller: peer.summary.clone(),
                    desired_access,
                    granted_access_bits: decision.granted_access_bits,
                }),
            ));
        }

        self.run_system_shutdown_command(request, controller, clock)
            .map_err(SupervisorSystemShutdownControlBodyError::supervisor)
    }

    pub fn run_checked_shutdown_control_body_with_response<C, P, A>(
        &mut self,
        body: &[u8],
        peer: &ControlPeer,
        control_security: &ControlSecurityDescriptor,
        access_checker: &mut A,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<SupervisorSystemShutdownControlBodyResponse, serde_json::Error>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ?Sized,
    {
        let result = self.run_checked_shutdown_control_body(
            body,
            peer,
            control_security,
            access_checker,
            controller,
            clock,
        );
        let response_line = system_shutdown_control_response_line(result.as_ref())?;
        Ok(match result {
            Ok(dispatch) => SupervisorSystemShutdownControlBodyResponse::Accepted {
                response_line,
                dispatch: Box::new(dispatch),
            },
            Err(error) => SupervisorSystemShutdownControlBodyResponse::Rejected {
                response_line,
                error,
            },
        })
    }
}
