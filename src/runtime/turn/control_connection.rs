use crate::boundary::{Clock, ProcessController, RealtimeClock, RegistryClient, ShutdownFinalizer};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::{
    Supervisor, SupervisorControlCommandDispatch, SupervisorControlConnectionTableTurn,
    SupervisorControlConnectionTableTurnError, SupervisorControlConnectionTurnContext,
    SupervisorControlFrameTurn,
};

use super::deadline::sync_deadline_timer;
use super::model::{
    RuntimeEventRegistrar, RuntimeShutdownDeadlineTimer, RuntimeShutdownEventContext,
    RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError,
};

pub(super) fn process_control_connection_event<I, D, C, P, F, A, R, G>(
    supervisor: &mut Supervisor,
    fd: i32,
    control_connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
    registry: Option<&mut G>,
    deadline_timer: &mut D,
    context: RuntimeShutdownEventContext<'_, C, P, F, A, R>,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    I: ControlConnectionIo,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    A: SystemAccessChecker + ServiceAccessChecker + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    G: RegistryClient,
{
    if control_connections.get(fd).is_none() {
        return Ok(RuntimeShutdownEventTurn::StaleControlConnection { fd });
    }
    let observed_at_ns = context
        .clock
        .monotonic_ns()
        .map_err(RuntimeShutdownEventTurnError::Clock)?;
    let turn = match supervisor.process_control_connection_table_turn(
        control_connections,
        fd,
        SupervisorControlConnectionTurnContext {
            control_security: context.control_security,
            access_checker: context.access_checker,
            controller: context.controller,
            clock: context.clock,
            registry: registry.map(|registry| registry as &mut dyn RegistryClient),
            max_read_bytes: context.control_limits.max_read_bytes,
            max_request_bytes: context.control_limits.max_request_bytes,
            observed_at_ns,
        },
    ) {
        Ok(turn) => turn,
        Err(SupervisorControlConnectionTableTurnError::MissingConnection { fd }) => {
            return Ok(RuntimeShutdownEventTurn::StaleControlConnection { fd });
        }
        Err(error) => return Err(RuntimeShutdownEventTurnError::ControlConnection(error)),
    };
    let deadline_timer_turn = if control_connection_started_shutdown(&turn) {
        Some(sync_deadline_timer(supervisor, deadline_timer)?)
    } else {
        None
    };

    Ok(RuntimeShutdownEventTurn::ControlConnection {
        fd,
        supervisor: Box::new(turn),
        deadline_timer: deadline_timer_turn,
    })
}

fn control_connection_started_shutdown(turn: &SupervisorControlConnectionTableTurn) -> bool {
    matches!(
        turn.turn.frame.as_ref().map(|frame| &frame.frame),
        Some(SupervisorControlFrameTurn::CommandAccepted {
            dispatch: Some(dispatch),
            ..
        }) if matches!(&**dispatch, SupervisorControlCommandDispatch::Shutdown(_)),
    )
}
