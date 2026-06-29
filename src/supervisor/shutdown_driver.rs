use crate::boundary::{ProcessController, ShutdownFinalizer};
use crate::shutdown::{ShutdownDeadline, ShutdownDeadlineKind, ShutdownFinalizationState};

use super::dispatch::SupervisorShutdownDriveDispatch;
use super::state::{Supervisor, SupervisorError};

impl Supervisor {
    pub fn next_shutdown_deadline(&self) -> Option<ShutdownDeadline> {
        let shutdown = self.shutdown.as_ref()?;
        let mut deadlines = Vec::new();

        if matches!(
            shutdown.finalization,
            ShutdownFinalizationState::WaitingForServices
        ) {
            deadlines.push(ShutdownDeadline {
                due_at_ns: shutdown.global_deadline_ns,
                kind: ShutdownDeadlineKind::GlobalTimeout,
            });
        }
        deadlines.extend(
            shutdown
                .stop_deadlines
                .iter()
                .map(|deadline| ShutdownDeadline {
                    due_at_ns: deadline.due_at_ns,
                    kind: ShutdownDeadlineKind::StopTimeout {
                        service: deadline.service.clone(),
                    },
                }),
        );
        deadlines.extend(
            shutdown
                .post_kill_deadlines
                .iter()
                .map(|deadline| ShutdownDeadline {
                    due_at_ns: deadline.due_at_ns,
                    kind: ShutdownDeadlineKind::PostKillTimeout {
                        service: deadline.service.clone(),
                    },
                }),
        );
        if let ShutdownFinalizationState::Failed {
            next_retry_at_ns, ..
        } = shutdown.finalization
        {
            deadlines.push(ShutdownDeadline {
                due_at_ns: next_retry_at_ns,
                kind: ShutdownDeadlineKind::FinalActionRetry,
            });
        }

        deadlines
            .into_iter()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline_sort_key(&deadline.kind)))
    }

    pub fn drive_shutdown<P, F>(
        &mut self,
        controller: &mut P,
        finalizer: &mut F,
        now_ns: u64,
    ) -> Result<Option<SupervisorShutdownDriveDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer + ?Sized,
    {
        if self.shutdown.is_none() {
            return Ok(None);
        }

        let timeout = self.process_due_shutdown_timeouts(controller, now_ns)?;
        let finalization = if shutdown_finalization_due(self, now_ns) {
            Some(self.finalize_shutdown(finalizer, now_ns)?)
        } else {
            None
        };

        if timeout.is_none() && finalization.is_none() {
            Ok(None)
        } else {
            Ok(Some(SupervisorShutdownDriveDispatch {
                timeout,
                finalization,
            }))
        }
    }
}

fn shutdown_finalization_due(supervisor: &Supervisor, now_ns: u64) -> bool {
    match supervisor.shutdown().map(|shutdown| &shutdown.finalization) {
        Some(ShutdownFinalizationState::Ready) => true,
        Some(ShutdownFinalizationState::Failed {
            next_retry_at_ns, ..
        }) => now_ns >= *next_retry_at_ns,
        _ => false,
    }
}

fn deadline_sort_key(kind: &ShutdownDeadlineKind) -> u8 {
    match kind {
        ShutdownDeadlineKind::StopTimeout { .. } => 0,
        ShutdownDeadlineKind::PostKillTimeout { .. } => 1,
        ShutdownDeadlineKind::GlobalTimeout => 2,
        ShutdownDeadlineKind::FinalActionRetry => 3,
    }
}
