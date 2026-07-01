use crate::boundary::ProcessController;
use crate::shutdown::ShutdownKind;

use super::dispatch::{SupervisorPowerButtonAction, SupervisorPowerButtonDispatch};
use super::state::{Supervisor, SupervisorError};

impl Supervisor {
    pub fn handle_power_button<P>(
        &mut self,
        controller: &mut P,
        observed_at_ns: u64,
    ) -> Result<SupervisorPowerButtonDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let action = if let Some(shutdown) = &self.shutdown {
            SupervisorPowerButtonAction::AlreadyInProgress {
                kind: shutdown.kind,
            }
        } else {
            self.begin_shutdown(ShutdownKind::Poweroff, controller, observed_at_ns)
                .map(Box::new)
                .map(SupervisorPowerButtonAction::Graceful)?
        };
        Ok(SupervisorPowerButtonDispatch { action })
    }
}
