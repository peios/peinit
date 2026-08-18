use crate::boundary::ShutdownDeadlineTimer;

use super::SupervisorLifecycleDeadline;
use crate::supervisor::state::{Supervisor, SupervisorError};

impl Supervisor {
    pub fn next_lifecycle_deadline(&self) -> Option<SupervisorLifecycleDeadline> {
        if self.shutdown().is_some() {
            return None;
        }

        [
            self.next_pre_start_check_timeout()
                .map(SupervisorLifecycleDeadline::from),
            self.next_pre_start_hook_timeout()
                .map(SupervisorLifecycleDeadline::from),
            self.next_post_start_hook_timeout()
                .map(SupervisorLifecycleDeadline::from),
            self.next_readiness_timeout()
                .map(SupervisorLifecycleDeadline::from),
            self.next_stop_timeout_deadline()
                .map(SupervisorLifecycleDeadline::from),
            self.next_reload_detection_deadline()
                .map(SupervisorLifecycleDeadline::from),
            self.next_reload_command_timeout()
                .map(SupervisorLifecycleDeadline::from),
            self.next_restart_backoff_deadline()
                .map(SupervisorLifecycleDeadline::from),
            self.next_health_check_timeout()
                .map(SupervisorLifecycleDeadline::from),
            self.next_watchdog_timeout()
                .map(SupervisorLifecycleDeadline::from),
            self.next_health_check_interval()
                .map(SupervisorLifecycleDeadline::from),
            self.next_cgroup_cleanup_deadline()
                .map(SupervisorLifecycleDeadline::from),
            self.boot_success
                .next_deadline(&self.services)
                .map(SupervisorLifecycleDeadline::from),
            self.boot_settle
                .next_deadline(&self.services)
                .map(SupervisorLifecycleDeadline::from),
        ]
        .into_iter()
        .flatten()
        .min_by(SupervisorLifecycleDeadline::cmp_schedule)
    }

    pub fn sync_lifecycle_deadline_timer<T>(
        &self,
        timer: &mut T,
    ) -> Result<SupervisorLifecycleDeadlineTimerTurn, SupervisorError>
    where
        T: ShutdownDeadlineTimer + ?Sized,
    {
        match self.next_lifecycle_deadline() {
            Some(deadline) => {
                timer
                    .arm_absolute_ns(deadline.due_at_ns)
                    .map_err(SupervisorError::Timer)?;
                Ok(SupervisorLifecycleDeadlineTimerTurn::Armed { deadline })
            }
            None => {
                timer.disarm().map_err(SupervisorError::Timer)?;
                Ok(SupervisorLifecycleDeadlineTimerTurn::Disarmed)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorLifecycleDeadlineTimerTurn {
    Armed {
        deadline: SupervisorLifecycleDeadline,
    },
    Disarmed,
}
