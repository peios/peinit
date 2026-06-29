mod abandoned_reset;
mod finish;
mod request;

use crate::boundary::Clock;
use crate::control::lifecycle::{LifecycleCommand, LifecycleCommandError, LifecycleCommandRequest};
use crate::security::TokenSummary;
use crate::service::runtime::ServiceState;

use self::abandoned_reset::service_is_abandoned;
use self::finish::finish_lifecycle_outcome;
use self::request::{admit_supervisor_lifecycle_command, allocate_request_id};
use super::dispatch::SupervisorLifecycleDispatch;
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

impl Supervisor {
    pub fn run_lifecycle_command<C>(
        &mut self,
        command: LifecycleCommand,
        service: impl Into<String>,
        caller: Option<TokenSummary>,
        clock: &mut C,
    ) -> Result<SupervisorLifecycleDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
    {
        if let Some(shutdown) = &self.shutdown {
            return Err(SupervisorError::Shutdown(
                crate::shutdown::ShutdownError::AlreadyInProgress {
                    kind: shutdown.kind,
                },
            ));
        }
        let service = service.into();
        reject_abandoned_reset_without_controller(&self.services, command, &service)?;
        let created_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let mut work = SupervisorWork::from_supervisor(self);
        let request_id = allocate_request_id(&mut work, created_at_ns)?;
        let outcome = admit_supervisor_lifecycle_command(
            &mut work,
            LifecycleCommandRequest {
                id: request_id,
                command,
                service,
                caller,
                created_at_ns,
            },
        )?;
        finish_lifecycle_outcome(
            self,
            work,
            outcome,
            self.settings.phase2.max_parallel_starts,
            created_at_ns,
        )
    }
}

fn reject_abandoned_reset_without_controller(
    services: &crate::service::ServiceTable,
    command: LifecycleCommand,
    service: &str,
) -> Result<(), SupervisorError> {
    if command == LifecycleCommand::Reset && service_is_abandoned(services, service)? {
        return Err(SupervisorError::Lifecycle(
            LifecycleCommandError::InvalidState {
                service: service.to_string(),
                command,
                state: ServiceState::Abandoned,
            },
        ));
    }
    Ok(())
}
