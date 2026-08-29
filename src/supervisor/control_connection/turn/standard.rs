use crate::boundary::{Clock, ProcessController, RealtimeClock};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionReadTurn, ControlConnectionRecord,
    ControlConnectionWriteTurn,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::control_connection::SupervisorControlFrameContext;
use crate::supervisor::state::Supervisor;

use super::model::{
    SupervisorControlConnectionTurn, SupervisorControlConnectionTurnContext,
    SupervisorControlConnectionTurnError,
};
use super::should_close_connection;

impl Supervisor {
    pub fn process_control_connection_turn<I, C, P, A>(
        &mut self,
        connection: &mut ControlConnectionRecord<I>,
        context: SupervisorControlConnectionTurnContext<'_, '_, C, P, A>,
    ) -> Result<SupervisorControlConnectionTurn, SupervisorControlConnectionTurnError>
    where
        I: ControlConnectionIo,
        C: Clock + RealtimeClock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ServiceAccessChecker + crate::submitted::JobAccessChecker + ?Sized,
    {
        let SupervisorControlConnectionTurnContext {
            control_security,
            access_checker,
            controller,
            clock,
            registry,
            max_read_bytes,
            max_request_bytes,
            observed_at_ns,
        } = context;

        let read = connection
            .read(max_read_bytes)
            .map_err(SupervisorControlConnectionTurnError::Read)?;
        let frame = if read == ControlConnectionReadTurn::Eof
            || connection.state().pending_wait().is_some()
        {
            None
        } else {
            let (peer, state) = connection.peer_and_state_mut();
            Some(
                self.process_next_control_connection_frame(
                    state,
                    SupervisorControlFrameContext {
                        peer,
                        control_security,
                        access_checker,
                        controller,
                        clock,
                        registry,
                        max_request_bytes,
                    },
                )
                .map_err(SupervisorControlConnectionTurnError::Frame)?,
            )
        };
        let write = connection
            .flush()
            .map_err(SupervisorControlConnectionTurnError::Write)?;
        let close_connection = should_close_connection(&read, &write);
        if control_connection_observed_activity(&read, frame.as_ref(), &write) {
            connection.state_mut().mark_activity(observed_at_ns);
        }

        Ok(SupervisorControlConnectionTurn {
            read,
            frame,
            write,
            close_connection,
        })
    }
}

pub(super) fn control_connection_observed_activity<T>(
    read: &ControlConnectionReadTurn,
    frame: Option<&T>,
    write: &ControlConnectionWriteTurn,
) -> bool {
    matches!(read, ControlConnectionReadTurn::Bytes { read_bytes, .. } if *read_bytes > 0)
        || frame.is_some()
        || matches!(
            write,
            ControlConnectionWriteTurn::Complete { written, .. }
                | ControlConnectionWriteTurn::Partial { written, .. } if *written > 0
        )
}
