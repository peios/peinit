use crate::boundary::{Clock, ProcessController, RealtimeClock};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::state::Supervisor;

use super::{
    SupervisorControlConnectionTurn, SupervisorControlConnectionTurnContext,
    SupervisorControlConnectionTurnError, SupervisorShutdownControlConnectionTurn,
    SupervisorShutdownControlConnectionTurnContext, SupervisorShutdownControlConnectionTurnError,
};

impl Supervisor {
    pub fn process_control_connection_table_turn<I, C, P, A>(
        &mut self,
        connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
        fd: i32,
        context: SupervisorControlConnectionTurnContext<'_, '_, C, P, A>,
    ) -> Result<SupervisorControlConnectionTableTurn, SupervisorControlConnectionTableTurnError>
    where
        I: ControlConnectionIo,
        C: Clock + RealtimeClock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ServiceAccessChecker + crate::submitted::JobAccessChecker + ?Sized,
    {
        let turn = {
            let connection = connections
                .get_mut(fd)
                .ok_or(SupervisorControlConnectionTableTurnError::MissingConnection { fd })?;
            self.process_control_connection_turn(connection, context)
                .map_err(SupervisorControlConnectionTableTurnError::Connection)?
        };
        let removed = if turn.close_connection {
            connections.remove(fd).is_some()
        } else {
            false
        };

        Ok(SupervisorControlConnectionTableTurn {
            fd,
            turn,
            removed,
            active_connections: connections.len(),
        })
    }

    pub fn process_shutdown_control_connection_table_turn<I, C, P, A>(
        &mut self,
        connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
        fd: i32,
        context: SupervisorShutdownControlConnectionTurnContext<'_, C, P, A>,
    ) -> Result<
        SupervisorShutdownControlConnectionTableTurn,
        SupervisorShutdownControlConnectionTableTurnError,
    >
    where
        I: ControlConnectionIo,
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
        A: SystemAccessChecker + ?Sized,
    {
        let turn = {
            let connection = connections.get_mut(fd).ok_or(
                SupervisorShutdownControlConnectionTableTurnError::MissingConnection { fd },
            )?;
            self.process_shutdown_control_connection_turn(connection, context)
                .map_err(SupervisorShutdownControlConnectionTableTurnError::Connection)?
        };
        let removed = if turn.close_connection {
            connections.remove(fd).is_some()
        } else {
            false
        };

        Ok(SupervisorShutdownControlConnectionTableTurn {
            fd,
            turn,
            removed,
            active_connections: connections.len(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlConnectionTableTurn {
    pub fd: i32,
    pub turn: SupervisorControlConnectionTurn,
    pub removed: bool,
    pub active_connections: usize,
}

#[derive(Debug)]
pub enum SupervisorControlConnectionTableTurnError {
    MissingConnection { fd: i32 },
    Connection(SupervisorControlConnectionTurnError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownControlConnectionTableTurn {
    pub fd: i32,
    pub turn: SupervisorShutdownControlConnectionTurn,
    pub removed: bool,
    pub active_connections: usize,
}

#[derive(Debug)]
pub enum SupervisorShutdownControlConnectionTableTurnError {
    MissingConnection { fd: i32 },
    Connection(SupervisorShutdownControlConnectionTurnError),
}
