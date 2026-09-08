//! Matching a forked last-run write back to its outcome.
//!
//! §9.1 requires the timer last-run write to be asynchronous, because peinit's
//! event loop cannot wait on registryd — a service peinit supervises. A fork is
//! how peinit gets that, and the child carries the result in its exit status.
//!
//! Nothing looked at it. The generic PID 1 reaper treats a child it does not
//! recognise as untracked and discards its status, so a persistent timer whose
//! timestamp writes kept failing produced a spurious catch-up run on every
//! boot, and nothing anywhere said why (PEI-369).
//!
//! This is a small ring of outstanding writes keyed by pid. It is not the job
//! machinery: a last-run write is not a supervised job, has no operation and
//! no service state, and giving it a job record would put it in `svctl status`
//! and the shutdown waves where it does not belong.

use std::collections::VecDeque;

use crate::boundary::ChildExitStatus;

/// How many outstanding writes to remember.
///
/// A child `_exit`s the moment the write returns, so in practice at most a
/// handful are in flight. The cap is not a tuning parameter, it is a promise
/// that a child which somehow never gets reaped cannot grow this without
/// bound.
const MAX_OUTSTANDING: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct OutstandingWrite {
    pid: u32,
    service: String,
    schedule: String,
}

/// A last-run write whose child reported failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FailedTimerLastRunWrite {
    pub service: String,
    pub schedule: String,
}

#[derive(Debug, Default)]
pub(super) struct TimerLastRunWrites {
    outstanding: VecDeque<OutstandingWrite>,
}

impl TimerLastRunWrites {
    pub(super) fn record(&mut self, pid: u32, service: String, schedule: String) {
        if self.outstanding.len() >= MAX_OUTSTANDING {
            self.outstanding.pop_front();
        }
        self.outstanding.push_back(OutstandingWrite {
            pid,
            service,
            schedule,
        });
    }

    /// Claim a reaped pid, reporting the write if it failed.
    ///
    /// `None` for a pid this does not know, which is every other untracked
    /// child, and `None` for a clean exit — the write worked and there is
    /// nothing to say.
    pub(super) fn claim(
        &mut self,
        pid: u32,
        status: ChildExitStatus,
    ) -> Option<FailedTimerLastRunWrite> {
        let index = self.outstanding.iter().position(|write| write.pid == pid)?;
        let write = self.outstanding.remove(index)?;
        match status {
            ChildExitStatus::Exited { code: 0 } => None,
            _ => Some(FailedTimerLastRunWrite {
                service: write.service,
                schedule: write.schedule,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_exit_is_claimed_and_says_nothing() {
        let mut writes = TimerLastRunWrites::default();
        writes.record(4242, "backup".to_string(), "daily UTC".to_string());

        assert_eq!(
            writes.claim(4242, ChildExitStatus::Exited { code: 0 }),
            None,
        );
        // Claimed, so a pid is never matched twice.
        assert_eq!(
            writes.claim(4242, ChildExitStatus::Exited { code: 0 }),
            None,
        );
    }

    // The whole point: a failed write used to exist only as an exit status the
    // reaper discarded, so a persistent timer that could not record its
    // last-run timestamp ran its catch-up on every boot with no explanation.
    #[test]
    fn a_failed_write_names_the_timer_it_belonged_to() {
        let mut writes = TimerLastRunWrites::default();
        writes.record(4242, "backup".to_string(), "daily UTC".to_string());

        assert_eq!(
            writes.claim(4242, ChildExitStatus::Exited { code: 1 }),
            Some(FailedTimerLastRunWrite {
                service: "backup".to_string(),
                schedule: "daily UTC".to_string(),
            }),
        );
    }

    /// A child killed by a signal did not complete the write either.
    #[test]
    fn a_signalled_child_counts_as_a_failed_write() {
        let mut writes = TimerLastRunWrites::default();
        writes.record(4242, "backup".to_string(), "daily UTC".to_string());

        assert!(
            writes
                .claim(
                    4242,
                    ChildExitStatus::Signaled {
                        signal: 9,
                        core_dumped: false,
                    },
                )
                .is_some(),
        );
    }

    /// Every other untracked child passes straight through.
    #[test]
    fn an_unknown_pid_is_not_claimed() {
        let mut writes = TimerLastRunWrites::default();
        writes.record(4242, "backup".to_string(), "daily UTC".to_string());

        assert_eq!(
            writes.claim(9999, ChildExitStatus::Exited { code: 1 }),
            None
        );
    }

    /// A child that somehow never gets reaped must not grow the ring.
    #[test]
    fn outstanding_writes_are_bounded() {
        let mut writes = TimerLastRunWrites::default();
        for pid in 0..(MAX_OUTSTANDING as u32 * 2) {
            writes.record(pid, "backup".to_string(), "daily UTC".to_string());
        }

        assert_eq!(writes.outstanding.len(), MAX_OUTSTANDING);
        // The oldest are the ones dropped.
        assert_eq!(writes.claim(0, ChildExitStatus::Exited { code: 1 }), None,);
        assert!(
            writes
                .claim(
                    MAX_OUTSTANDING as u32 * 2 - 1,
                    ChildExitStatus::Exited { code: 1 }
                )
                .is_some(),
        );
    }
}
