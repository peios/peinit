use crate::boundary::ShutdownDeadlineTimer;

use super::SupervisorLifecycleDeadline;
use crate::supervisor::state::{Supervisor, SupervisorError};

impl Supervisor {
    /// The next lifecycle deadline, as the timer should arm it.
    ///
    /// A deadline under a holdoff is reported no earlier than the holdoff's
    /// end, so a deadline that raised and could not be removed is acted on
    /// at most once per interval instead of on every turn (PEI-1267).
    pub fn next_lifecycle_deadline(&self) -> Option<SupervisorLifecycleDeadline> {
        let mut deadline = self.next_scheduled_lifecycle_deadline()?;
        deadline.due_at_ns = self.held_due_at_ns(&deadline);
        Some(deadline)
    }

    /// When a scheduled deadline may be acted on: when it is due, or when
    /// its holdoff ends if that is later.
    pub(super) fn held_due_at_ns(&self, deadline: &SupervisorLifecycleDeadline) -> u64 {
        self.lifecycle_deadline_holdoffs
            .iter()
            .find(|holdoff| holdoff.deadline == *deadline)
            .map_or(deadline.due_at_ns, |holdoff| {
                deadline.due_at_ns.max(holdoff.not_before_ns)
            })
    }

    /// The next lifecycle deadline as its store holds it, holdoffs aside.
    pub(super) fn next_scheduled_lifecycle_deadline(&self) -> Option<SupervisorLifecycleDeadline> {
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
            self.next_submitted_job_deadline(),
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
