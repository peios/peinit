use crate::boundary::{Clock, ProcessController, RealtimeClock, RegistryClient};
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
            mut registry,
            max_read_bytes,
            max_request_bytes,
            observed_at_ns,
        } = context;

        let read = connection
            .read(max_read_bytes)
            .map_err(SupervisorControlConnectionTurnError::Read)?;
        let mut frames = Vec::new();
        // One read can carry several frames, and a frame can be left over
        // from an earlier read that a wait held back. Keep going until the
        // buffer holds no complete frame, a wait blocks the connection, or a
        // rejection has scheduled its close; a readable event is not coming
        // for bytes that are already here (PEI-1073).
        while read != ControlConnectionReadTurn::Eof
            && connection.state().pending_wait().is_none()
            && !connection.state().close_after_write()
            && (frames.is_empty() || connection.state().read_buffer().holds_complete_frame())
        {
            let (peer, state) = connection.peer_and_state_mut();
            let frame = self
                .process_next_control_connection_frame(
                    state,
                    SupervisorControlFrameContext {
                        peer,
                        control_security,
                        access_checker,
                        controller,
                        clock,
                        registry: registry
                            .as_deref_mut()
                            .map(|registry| registry as &mut dyn RegistryClient),
                        max_request_bytes,
                    },
                )
                .map_err(SupervisorControlConnectionTurnError::Frame)?;
            frames.push(frame);
        }
        let write = connection
            .flush()
            .map_err(SupervisorControlConnectionTurnError::Write)?;
        let close_connection = should_close_connection(&read, &write);
        if control_connection_observed_activity(&read, &frames, &write) {
            connection.state_mut().mark_activity(observed_at_ns);
        }

        Ok(SupervisorControlConnectionTurn {
            read,
            frames,
            write,
            close_connection,
        })
    }
}

pub(super) fn control_connection_observed_activity<T>(
    read: &ControlConnectionReadTurn,
    frames: &[T],
    write: &ControlConnectionWriteTurn,
) -> bool {
    matches!(read, ControlConnectionReadTurn::Bytes { read_bytes, .. } if *read_bytes > 0)
        || !frames.is_empty()
        || matches!(
            write,
            ControlConnectionWriteTurn::Complete { written, .. }
                | ControlConnectionWriteTurn::Partial { written, .. } if *written > 0
        )
}
