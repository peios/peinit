use crate::boundary::{Clock, ProcessController, RealtimeClock};
use crate::control::connection::{ControlOperationWait, ControlPendingWait};
use crate::control::lifecycle::{LifecycleCommand, LifecycleCommandOutcome};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::ControlPeer;
use crate::control::wire::{
    ControlCommand, control_lifecycle_ack_response_line, control_status_response_line,
};
use crate::ids::OperationId;
use crate::supervisor::{Supervisor, SupervisorLifecycleDispatch};

use super::{
    SupervisorControlCommandBodyError, SupervisorControlCommandBodyResponse,
    SupervisorControlCommandDispatch,
};
use crate::supervisor::control_command::time::response_time_projection;

impl Supervisor {
    pub(super) fn run_control_lifecycle_command<C, P, A>(
        &mut self,
        command: ControlCommand,
        service: &str,
        wait: bool,
        context: ControlLifecycleCommandContext<'_, C, P, A>,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        C: Clock + RealtimeClock + ?Sized,
        P: ProcessController + ?Sized,
        A: ServiceAccessChecker + ?Sized,
    {
        let ControlLifecycleCommandContext {
            peer,
            access_checker,
            controller,
            clock,
        } = context;
        self.check_service_command_access(peer, access_checker, service, command)?;
        let lifecycle_command = lifecycle_command_from_control(command)?;
        let dispatch = self
            .run_lifecycle_command_with_process_controller(
                lifecycle_command,
                service,
                Some(peer.summary.clone()),
                controller,
                clock,
            )
            .map_err(SupervisorControlCommandBodyError::supervisor)?;

        if matches!(
            dispatch.outcome,
            LifecycleCommandOutcome::Already(_) | LifecycleCommandOutcome::Noop(_)
        ) {
            let view = self
                .service_status(service)
                .map_err(SupervisorControlCommandBodyError::Query)?;
            let time = response_time_projection(clock)?;
            return Ok(SupervisorControlCommandBodyResponse::Accepted {
                response_line: Some(
                    control_status_response_line(&view, time)
                        .map_err(SupervisorControlCommandBodyError::serialize)?,
                ),
                dispatch: Some(Box::new(SupervisorControlCommandDispatch::Lifecycle(
                    Box::new(dispatch),
                ))),
                wait: None,
                access_denials: Vec::new(),
                job_access_denials: Vec::new(),
            });
        }

        let operation_id = lifecycle_operation_id(&dispatch)
            .ok_or(SupervisorControlCommandBodyError::MissingLifecycleOperation)?;
        if wait && !self.operation_is_terminal(operation_id) {
            return Ok(SupervisorControlCommandBodyResponse::Accepted {
                response_line: None,
                dispatch: Some(Box::new(SupervisorControlCommandDispatch::Lifecycle(
                    Box::new(dispatch),
                ))),
                wait: Some(ControlPendingWait::Operation(ControlOperationWait {
                    operation_id,
                    service: service.to_string(),
                })),
                access_denials: Vec::new(),
                job_access_denials: Vec::new(),
            });
        }

        let view = self
            .service_status(service)
            .map_err(SupervisorControlCommandBodyError::Query)?;
        let lifecycle_warnings = lifecycle_warnings(&dispatch, &view.lifecycle_warnings);
        Ok(SupervisorControlCommandBodyResponse::Accepted {
            response_line: Some(
                control_lifecycle_ack_response_line(
                    Some(operation_id),
                    service,
                    view.state,
                    view.cause,
                    &lifecycle_warnings,
                )
                .map_err(SupervisorControlCommandBodyError::serialize)?,
            ),
            dispatch: Some(Box::new(SupervisorControlCommandDispatch::Lifecycle(
                Box::new(dispatch),
            ))),
            wait: None,
            access_denials: Vec::new(),
            job_access_denials: Vec::new(),
        })
    }

    fn operation_is_terminal(&self, operation_id: OperationId) -> bool {
        self.operations
            .get(operation_id)
            .is_some_and(|operation| operation.state.is_terminal())
    }
}

pub(super) struct ControlLifecycleCommandContext<'a, C, P, A>
where
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    A: ServiceAccessChecker + ?Sized,
{
    pub peer: &'a ControlPeer,
    pub access_checker: &'a mut A,
    pub controller: &'a mut P,
    pub clock: &'a mut C,
}

fn lifecycle_warnings(dispatch: &SupervisorLifecycleDispatch, projected: &[String]) -> Vec<String> {
    dispatch
        .lifecycle_warnings
        .iter()
        .chain(projected.iter())
        .cloned()
        .collect()
}

fn lifecycle_command_from_control(
    command: ControlCommand,
) -> Result<LifecycleCommand, SupervisorControlCommandBodyError> {
    match command {
        ControlCommand::Start => Ok(LifecycleCommand::Start),
        ControlCommand::Stop => Ok(LifecycleCommand::Stop),
        ControlCommand::Restart => Ok(LifecycleCommand::Restart),
        ControlCommand::Reload => Ok(LifecycleCommand::Reload),
        ControlCommand::Reset => Ok(LifecycleCommand::Reset),
        _ => Err(SupervisorControlCommandBodyError::InvalidArguments),
    }
}

fn lifecycle_operation_id(dispatch: &SupervisorLifecycleDispatch) -> Option<OperationId> {
    match &dispatch.outcome {
        LifecycleCommandOutcome::OperationAccepted(outcome) => Some(outcome.returned_operation_id),
        LifecycleCommandOutcome::OnDemandStart(outcome) => {
            Some(outcome.requested_operation.returned_operation_id)
        }
        LifecycleCommandOutcome::SynchronousClear(outcome) => {
            Some(outcome.request.returned_operation_id)
        }
        LifecycleCommandOutcome::Already(_) | LifecycleCommandOutcome::Noop(_) => None,
    }
}
