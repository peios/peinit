use crate::boundary::ShutdownDeadlineTimer;
use crate::shutdown::{ShutdownDeadline, ShutdownError};

use super::state::{Supervisor, SupervisorError};

impl Supervisor {
    pub fn sync_shutdown_deadline_timer<T>(
        &self,
        timer: &mut T,
    ) -> Result<SupervisorShutdownDeadlineTimerTurn, SupervisorError>
    where
        T: ShutdownDeadlineTimer + ?Sized,
    {
        match self.next_shutdown_deadline() {
            Some(deadline) => {
                timer
                    .arm_absolute_ns(deadline.due_at_ns)
                    .map_err(|error| SupervisorError::Shutdown(ShutdownError::Boundary(error)))?;
                Ok(SupervisorShutdownDeadlineTimerTurn::Armed { deadline })
            }
            None => {
                timer
                    .disarm()
                    .map_err(|error| SupervisorError::Shutdown(ShutdownError::Boundary(error)))?;
                Ok(SupervisorShutdownDeadlineTimerTurn::Disarmed)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorShutdownDeadlineTimerTurn {
    Armed { deadline: ShutdownDeadline },
    Disarmed,
}
