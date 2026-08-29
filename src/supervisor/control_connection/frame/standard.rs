use crate::boundary::{Clock, ProcessController, RealtimeClock};
use crate::control::connection::ControlConnectionState;
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::control::wire::{ControlConnectionBuffer, ControlFrameDecision};
use crate::supervisor::control_command::SupervisorControlCommandBodyContext;
use crate::supervisor::state::Supervisor;

use super::frame_reject_response_line;
use super::model::{
    SupervisorControlConnectionFrameTurn, SupervisorControlFrameContext,
    SupervisorControlFrameTurn, SupervisorControlFrameTurnError,
};

impl Supervisor {
    pub fn process_next_control_connection_frame<C, P, A>(
        &mut self,
        connection: &mut ControlConnectionState,
        context: SupervisorControlFrameContext<'_, '_, C, P, A>,
    ) -> Result<SupervisorControlConnectionFrameTurn, SupervisorControlFrameTurnError>
    where
        C: Clock + RealtimeClock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ServiceAccessChecker + crate::submitted::JobAccessChecker + ?Sized,
    {
        let frame = self.process_next_control_frame(connection.read_buffer_mut(), context)?;
        if let Some(response_line) = frame.response_line() {
            connection.enqueue_response(response_line, frame.close_after_response());
        }
        if let Some(wait) = frame.wait() {
            connection.set_pending_wait(wait.clone());
        }
        Ok(SupervisorControlConnectionFrameTurn {
            frame,
            pending_write_bytes: connection.pending_write_bytes(),
            close_after_write: connection.close_after_write(),
        })
    }

    pub fn process_next_control_frame<C, P, A>(
        &mut self,
        buffer: &mut ControlConnectionBuffer,
        context: SupervisorControlFrameContext<'_, '_, C, P, A>,
    ) -> Result<SupervisorControlFrameTurn, SupervisorControlFrameTurnError>
    where
        C: Clock + RealtimeClock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ServiceAccessChecker + crate::submitted::JobAccessChecker + ?Sized,
    {
        let SupervisorControlFrameContext {
            peer,
            control_security,
            access_checker,
            controller,
            clock,
            registry,
            max_request_bytes,
        } = context;

        match buffer.frame_decision(max_request_bytes) {
            ControlFrameDecision::Incomplete => Ok(SupervisorControlFrameTurn::Incomplete {
                buffered_bytes: buffer.len(),
            }),
            ControlFrameDecision::Reject { reason } => {
                let response_line = frame_reject_response_line(reason)
                    .map_err(SupervisorControlFrameTurnError::from)?;
                buffer.clear();
                Ok(SupervisorControlFrameTurn::RejectedFrame {
                    reason,
                    response_line,
                    close_after_response: true,
                })
            }
            ControlFrameDecision::Complete { body, consumed } => {
                buffer
                    .consume(consumed)
                    .map_err(SupervisorControlFrameTurnError::Buffer)?;
                let response = self
                    .run_checked_control_body_with_response(
                        &body,
                        SupervisorControlCommandBodyContext {
                            peer,
                            control_security,
                            access_checker,
                            controller,
                            clock,
                            registry,
                        },
                    )
                    .map_err(SupervisorControlFrameTurnError::from)?;
                Ok(match response {
                    crate::supervisor::SupervisorControlCommandBodyResponse::Accepted {
                        response_line,
                        dispatch,
                        wait,
                        access_denials,
                        job_access_denials,
                    } => SupervisorControlFrameTurn::CommandAccepted {
                        response_line,
                        dispatch,
                        wait,
                        access_denials,
                        job_access_denials,
                        remaining_bytes: buffer.len(),
                    },
                    crate::supervisor::SupervisorControlCommandBodyResponse::Rejected {
                        response_line,
                        error,
                    } => SupervisorControlFrameTurn::CommandRejected {
                        response_line,
                        error,
                        remaining_bytes: buffer.len(),
                    },
                })
            }
        }
    }
}
