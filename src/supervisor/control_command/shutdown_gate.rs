use crate::control::wire::ControlCommand;
use crate::shutdown::ShutdownError;
use crate::supervisor::{Supervisor, SupervisorError};

use super::SupervisorControlCommandBodyError;

impl Supervisor {
    pub(super) fn reject_non_query_command_during_shutdown(
        &self,
        command: ControlCommand,
    ) -> Result<(), SupervisorControlCommandBodyError> {
        if is_shutdown_allowed_query(command) {
            return Ok(());
        }
        let Some(shutdown) = self.shutdown() else {
            return Ok(());
        };
        Err(SupervisorControlCommandBodyError::supervisor(
            SupervisorError::Shutdown(ShutdownError::AlreadyInProgress {
                kind: shutdown.kind,
            }),
        ))
    }
}

fn is_shutdown_allowed_query(command: ControlCommand) -> bool {
    matches!(
        command,
        ControlCommand::Status | ControlCommand::List | ControlCommand::OperationStatus
    )
}
