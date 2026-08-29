use std::collections::VecDeque;
use std::os::fd::OwnedFd;

use crate::ids::JobId;
use crate::jobs::wire::JobsWaitCondition;

/// A message held on a connection until something about a job happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobsPendingWait {
    /// A `submit` answered when the job leaves `created`.
    Submit { job_id: JobId },
    /// A `wait` answered when its condition holds.
    Wait {
        job_id: JobId,
        condition: JobsWaitCondition,
    },
    /// A `stop` with `wait: true`, answered when the job is terminal.
    Stop { job_id: JobId },
}

impl JobsPendingWait {
    pub fn job_id(self) -> JobId {
        match self {
            Self::Submit { job_id } | Self::Wait { job_id, .. } | Self::Stop { job_id } => job_id,
        }
    }
}

/// One record queued for a connection. A descriptor attached to it is a
/// duplicate owned by the queue, so a job ending between enqueue and send
/// cannot leave a dangling number in the record.
#[derive(Debug)]
pub struct JobsOutgoing {
    pub bytes: Vec<u8>,
    pub fd: Option<OwnedFd>,
}

#[derive(Debug, Default)]
pub struct JobsConnectionState {
    pending_wait: Option<JobsPendingWait>,
    outgoing: VecDeque<JobsOutgoing>,
    close_after_write: bool,
    last_activity_ns: Option<u64>,
}

impl JobsConnectionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&mut self, bytes: Vec<u8>, fd: Option<OwnedFd>, close_after: bool) {
        self.outgoing.push_back(JobsOutgoing { bytes, fd });
        self.close_after_write |= close_after;
    }

    pub fn front(&self) -> Option<&JobsOutgoing> {
        self.outgoing.front()
    }

    pub fn pop_front(&mut self) -> Option<JobsOutgoing> {
        self.outgoing.pop_front()
    }

    pub fn pending_messages(&self) -> usize {
        self.outgoing.len()
    }

    pub fn close_after_write(&self) -> bool {
        self.close_after_write
    }

    pub fn mark_close_after_write(&mut self) {
        self.close_after_write = true;
    }

    pub fn pending_wait(&self) -> Option<JobsPendingWait> {
        self.pending_wait
    }

    pub fn set_pending_wait(&mut self, wait: JobsPendingWait) {
        self.pending_wait = Some(wait);
    }

    pub fn clear_pending_wait(&mut self) -> Option<JobsPendingWait> {
        self.pending_wait.take()
    }

    pub fn mark_activity(&mut self, observed_at_ns: u64) {
        self.last_activity_ns = Some(observed_at_ns);
    }

    /// Idle means nothing in flight: no wait, nothing queued (PSPU §7.3).
    pub fn idle_deadline_ns(&self, timeout_secs: u64) -> Option<u64> {
        if self.pending_wait.is_some() || !self.outgoing.is_empty() {
            return None;
        }
        self.last_activity_ns
            .map(|last| last.saturating_add(timeout_secs.saturating_mul(1_000_000_000)))
    }

    pub fn idle_timeout_expired(&self, now_ns: u64, timeout_secs: u64) -> bool {
        self.idle_deadline_ns(timeout_secs)
            .is_some_and(|deadline_ns| now_ns >= deadline_ns)
    }
}
