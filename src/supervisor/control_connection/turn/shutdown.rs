use crate::boundary::{Clock, ProcessController};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionReadTurn, ControlConnectionRecord,
};
use crate::control::system::SystemAccessChecker;
use crate::supervisor::control_connection::SupervisorShutdownControlFrameContext;
use crate::supervisor::state::Supervisor;

use super::model::{
    SupervisorShutdownControlConnectionTurn, SupervisorShutdownControlConnectionTurnContext,
    SupervisorShutdownControlConnectionTurnError,
};
use super::should_close_connection;
use super::standard::control_connection_observed_activity;

impl Supervisor {
    pub fn process_shutdown_control_connection_turn<I, C, P, A>(
        &mut self,
        connection: &mut ControlConnectionRecord<I>,
        context: SupervisorShutdownControlConnectionTurnContext<'_, C, P, A>,
    ) -> Result<SupervisorShutdownControlConnectionTurn, SupervisorShutdownControlConnectionTurnError>
    where
        I: ControlConnectionIo,
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ?Sized,
    {
        let SupervisorShutdownControlConnectionTurnContext {
            control_security,
            access_checker,
            controller,
            clock,
            max_read_bytes,
            max_request_bytes,
            observed_at_ns,
        } = context;

        let read = connection
            .read(max_read_bytes)
            .map_err(SupervisorShutdownControlConnectionTurnError::Read)?;
        let frame = if read == ControlConnectionReadTurn::Eof {
            None
        } else {
            let (peer, state) = connection.peer_and_state_mut();
            Some(
                self.process_next_shutdown_control_connection_frame(
                    state,
                    SupervisorShutdownControlFrameContext {
                        peer,
                        control_security,
                        access_checker,
                        controller,
                        clock,
                        max_request_bytes,
                    },
                )
                .map_err(SupervisorShutdownControlConnectionTurnError::Frame)?,
            )
        };
        let write = connection
            .flush()
            .map_err(SupervisorShutdownControlConnectionTurnError::Write)?;
        let close_connection = should_close_connection(&read, &write);
        if control_connection_observed_activity(&read, frame.as_slice(), &write) {
            connection.state_mut().mark_activity(observed_at_ns);
        }

        Ok(SupervisorShutdownControlConnectionTurn {
            read,
            frame,
            write,
            close_connection,
        })
    }
}
