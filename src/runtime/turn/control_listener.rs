use crate::control::connection::{
    ControlConnectionAcceptTurn, ControlConnectionIo, ControlConnectionRecord,
    ControlConnectionTable, ControlListener, accept_control_connection,
    accept_control_connection_at,
};
use crate::runtime::RuntimeEventSource;

use super::model::{
    RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};

pub(super) fn process_control_listener_event<I, L, R>(
    control_listener: &mut L,
    control_connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
    registrar: &mut R,
    accepted_at_ns: Option<u64>,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    I: ControlConnectionIo,
    L: ControlListener<Connection = I> + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    let accept = if accepted_at_ns.is_some() {
        accept_control_connection_at(control_listener, control_connections, accepted_at_ns)
    } else {
        accept_control_connection(control_listener, control_connections)
    }
    .map_err(RuntimeShutdownEventTurnError::ControlAccept)?;
    let registration = match &accept {
        ControlConnectionAcceptTurn::Accepted { fd, .. } => {
            let fd = *fd;
            let source = RuntimeEventSource::control_connection(fd).map_err(|error| {
                RuntimeShutdownEventTurnError::ControlRegistration(
                    RuntimeEventRegistrationError::Register {
                        fd,
                        source: RuntimeEventSource::ControlListener,
                        message: format!("{error:?}"),
                    },
                )
            })?;
            if let Err(error) = registrar.register_source(fd, source) {
                control_connections.remove(fd);
                return Err(RuntimeShutdownEventTurnError::ControlRegistration(error));
            }
            Some(source)
        }
        ControlConnectionAcceptTurn::RejectedAtSocket { .. }
        | ControlConnectionAcceptTurn::PeerRejected { .. }
        | ControlConnectionAcceptTurn::WouldBlock => None,
    };

    Ok(RuntimeShutdownEventTurn::ControlListener {
        accept,
        registration,
    })
}
