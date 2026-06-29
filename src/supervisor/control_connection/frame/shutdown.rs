use crate::boundary::{Clock, ProcessController};
use crate::control::connection::ControlConnectionState;
use crate::control::system::SystemAccessChecker;
use crate::control::wire::{ControlConnectionBuffer, ControlFrameDecision};
use crate::supervisor::state::Supervisor;
use crate::supervisor::system_shutdown::SupervisorSystemShutdownControlBodyResponse;

use super::frame_reject_response_line;
use super::model::{
    SupervisorControlConnectionFrameTurn, SupervisorControlFrameTurn,
    SupervisorControlFrameTurnError, SupervisorShutdownControlFrameContext,
};

impl Supervisor {
    pub fn process_next_shutdown_control_connection_frame<C, P, A>(
        &mut self,
        connection: &mut ControlConnectionState,
        context: SupervisorShutdownControlFrameContext<'_, C, P, A>,
    ) -> Result<SupervisorControlConnectionFrameTurn, SupervisorControlFrameTurnError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ?Sized,
    {
        let frame =
            self.process_next_shutdown_control_frame(connection.read_buffer_mut(), context)?;
        if let Some(response_line) = frame.response_line() {
            connection.enqueue_response(response_line, frame.close_after_response());
        }
        Ok(SupervisorControlConnectionFrameTurn {
            frame,
            pending_write_bytes: connection.pending_write_bytes(),
            close_after_write: connection.close_after_write(),
        })
    }

    pub fn process_next_shutdown_control_frame<C, P, A>(
        &mut self,
        buffer: &mut ControlConnectionBuffer,
        context: SupervisorShutdownControlFrameContext<'_, C, P, A>,
    ) -> Result<SupervisorControlFrameTurn, SupervisorControlFrameTurnError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ?Sized,
    {
        let SupervisorShutdownControlFrameContext {
            peer,
            control_security,
            access_checker,
            controller,
            clock,
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
                    .run_checked_shutdown_control_body_with_response(
                        &body,
                        peer,
                        control_security,
                        access_checker,
                        controller,
                        clock,
                    )
                    .map_err(SupervisorControlFrameTurnError::from)?;
                Ok(match response {
                    SupervisorSystemShutdownControlBodyResponse::Accepted {
                        response_line,
                        dispatch,
                    } => SupervisorControlFrameTurn::ShutdownAccepted {
                        response_line,
                        dispatch,
                        remaining_bytes: buffer.len(),
                    },
                    SupervisorSystemShutdownControlBodyResponse::Rejected {
                        response_line,
                        error,
                    } => SupervisorControlFrameTurn::ShutdownRejected {
                        response_line,
                        error,
                        remaining_bytes: buffer.len(),
                    },
                })
            }
        }
    }
}
