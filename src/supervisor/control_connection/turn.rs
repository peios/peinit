mod model;
mod shutdown;
mod standard;

pub use model::{
    SupervisorControlConnectionTurn, SupervisorControlConnectionTurnContext,
    SupervisorControlConnectionTurnError, SupervisorShutdownControlConnectionTurn,
    SupervisorShutdownControlConnectionTurnContext, SupervisorShutdownControlConnectionTurnError,
};

use crate::control::connection::{ControlConnectionReadTurn, ControlConnectionWriteTurn};

fn should_close_connection(
    read: &ControlConnectionReadTurn,
    write: &ControlConnectionWriteTurn,
) -> bool {
    match write {
        ControlConnectionWriteTurn::Partial { .. }
        | ControlConnectionWriteTurn::WouldBlock { .. } => false,
        ControlConnectionWriteTurn::Idle { close_after_write }
        | ControlConnectionWriteTurn::Complete {
            close_after_write, ..
        } => read == &ControlConnectionReadTurn::Eof || *close_after_write,
    }
}
