use crate::boundary::{ProcessController, ShutdownFinalizer};
use crate::shutdown::{ShutdownDeadline, ShutdownDeadlineKind, ShutdownFinalizationState};

use super::dispatch::{SupervisorShutdownDriveDispatch, SupervisorShutdownFinalizationDispatch};
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
            self.due_submitted_job_deadlines(u64::MAX)
                .into_iter()
                .map(|deadline| ShutdownDeadline {
                    due_at_ns: deadline.due_at_ns,
                    kind: ShutdownDeadlineKind::SubmittedJob {
                        job_id: deadline.job_id,
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

    /// Process the shutdown work due at `now_ns`: the stop and post-kill
    /// timeouts, and — given a finalizer — the final action if it is due.
    ///
    /// Given `None`, the final action is left for
    /// [`Self::finalize_due_shutdown`]. The runtime drives its turn that way,
    /// so the turn's console output can be written before an action that
    /// does not return (PEI-827).
    pub fn drive_shutdown<P>(
        &mut self,
        controller: &mut P,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
        now_ns: u64,
    ) -> Result<Option<SupervisorShutdownDriveDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        if self.shutdown.is_none() {
            return Ok(None);
        }

        let timeout = self.process_due_shutdown_timeouts(controller, now_ns)?;
        let finalization = match finalizer {
            Some(finalizer) => self.finalize_due_shutdown(finalizer, now_ns)?,
            None => None,
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

    /// Run the final action if the shutdown is ready for it, or a failed one
    /// is due its retry; nothing otherwise.
    pub fn finalize_due_shutdown<F>(
        &mut self,
        finalizer: &mut F,
        now_ns: u64,
    ) -> Result<Option<SupervisorShutdownFinalizationDispatch>, SupervisorError>
    where
        F: ShutdownFinalizer + ?Sized,
    {
        if self.shutdown_finalization_due(now_ns) {
            Ok(Some(self.finalize_shutdown(finalizer, now_ns)?))
        } else {
            Ok(None)
        }
    }

    /// Whether [`Self::finalize_due_shutdown`] would act now.
    pub fn shutdown_finalization_due(&self, now_ns: u64) -> bool {
        match self.shutdown().map(|shutdown| &shutdown.finalization) {
            Some(ShutdownFinalizationState::Ready) => true,
            Some(ShutdownFinalizationState::Failed {
                next_retry_at_ns, ..
            }) => now_ns >= *next_retry_at_ns,
            _ => false,
        }
    }
}

fn deadline_sort_key(kind: &ShutdownDeadlineKind) -> u8 {
    match kind {
        ShutdownDeadlineKind::StopTimeout { .. } => 0,
        ShutdownDeadlineKind::PostKillTimeout { .. } => 1,
        ShutdownDeadlineKind::SubmittedJob { .. } => 1,
        ShutdownDeadlineKind::GlobalTimeout => 2,
        ShutdownDeadlineKind::FinalActionRetry => 3,
    }
}
