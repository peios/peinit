mod access;
mod error;
mod lifecycle;
mod model;
mod query;
mod reload_config;
mod shutdown_gate;
mod system;
mod time;

pub use error::SupervisorControlCommandBodyError;
pub use model::{
    SupervisorControlCommandBodyContext, SupervisorControlCommandBodyResponse,
    SupervisorControlCommandDispatch,
};

use crate::boundary::{Clock, ProcessController, RealtimeClock};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::control::wire::{ControlCommand, control_error_response_line, parse_control_request};
use crate::submitted::JobAccessChecker;

use super::state::Supervisor;
use lifecycle::ControlLifecycleCommandContext;

impl Supervisor {
    pub fn run_checked_control_body_with_response<C, P, A>(
        &mut self,
        body: &[u8],
        context: SupervisorControlCommandBodyContext<'_, '_, C, P, A>,
    ) -> Result<SupervisorControlCommandBodyResponse, serde_json::Error>
    where
        C: Clock + RealtimeClock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ServiceAccessChecker + JobAccessChecker + ?Sized,
    {
        let result = self.run_checked_control_body(body, context);
        Ok(match result {
            Ok(success) => success,
            Err(error) => {
                let (code, message) = error.response_error();
                SupervisorControlCommandBodyResponse::Rejected {
                    response_line: control_error_response_line(code, &message)?,
                    error,
                }
            }
        })
    }

    fn run_checked_control_body<C, P, A>(
        &mut self,
        body: &[u8],
        context: SupervisorControlCommandBodyContext<'_, '_, C, P, A>,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        C: Clock + RealtimeClock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ServiceAccessChecker + JobAccessChecker + ?Sized,
    {
        let SupervisorControlCommandBodyContext {
            peer,
            control_security,
            access_checker,
            controller,
            clock,
            registry,
        } = context;
        let parsed =
            parse_control_request(body).map_err(SupervisorControlCommandBodyError::Parse)?;
        self.reject_non_query_command_during_shutdown(parsed.command)?;

        match parsed.command {
            ControlCommand::Start
            | ControlCommand::Stop
            | ControlCommand::Restart
            | ControlCommand::Reload
            | ControlCommand::Reset => {
                let service = parsed
                    .service
                    .as_deref()
                    .ok_or(SupervisorControlCommandBodyError::InvalidArguments)?;
                self.run_control_lifecycle_command(
                    parsed.command,
                    service,
                    parsed.wait,
                    ControlLifecycleCommandContext {
                        peer,
                        access_checker,
                        controller,
                        clock,
                    },
                )
            }
            ControlCommand::Status => {
                self.run_control_status_query(&parsed, peer, access_checker, clock)
            }
            ControlCommand::List => self.run_control_list_query(peer, access_checker),
            ControlCommand::OperationStatus => {
                self.run_control_operation_status_query(&parsed, peer, access_checker, clock)
            }
            ControlCommand::Shutdown => self.run_control_shutdown_command(
                &parsed,
                peer,
                control_security,
                access_checker,
                controller,
                clock,
            ),
            ControlCommand::ReloadConfig => self.run_control_reload_config_command(
                peer,
                control_security,
                access_checker,
                registry,
            ),
            ControlCommand::JobStatus => {
                self.run_control_job_status(&parsed, peer, access_checker, clock)
            }
            ControlCommand::JobList => {
                self.run_control_job_list(&parsed, peer, access_checker, clock)
            }
            ControlCommand::JobStop => {
                self.run_control_job_stop(&parsed, peer, access_checker, controller, clock)
            }
        }
    }
}
