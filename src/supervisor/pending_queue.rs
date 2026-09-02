//! Draining a launch queue without trusting it.
//!
//! Each launch queue holds job ids awaiting a launch, and the job store holds
//! the records. Keeping the two in step was an invariant maintained by hand at
//! every path that finishes a `Created` job — and the penalty for missing one
//! was severe out of all proportion to the mistake: the drain looked the id up,
//! got `UnknownJob`, and that error left peinit's runtime loop, which is fatal
//! (PEI-605).
//!
//! So the queue is treated as a *hint* and the job store as the truth. An id
//! whose record has gone is dropped here rather than launched, which makes the
//! invariant unnecessary rather than merely unenforced. Callers count what was
//! dropped so a bookkeeping fault still shows up as a number instead of
//! vanishing entirely.

use std::collections::VecDeque;

use crate::ids::JobId;
use crate::job::JobStore;

/// The next queued job that still has a record, discarding those that do not.
///
/// Leaves the returned id at the front: every caller pops it itself, on a work
/// copy it may or may not commit, and moving that decision in here would change
/// what a failed launch leaves behind.
pub(in crate::supervisor) fn next_live_front(
    queue: &mut VecDeque<JobId>,
    jobs: &JobStore,
    dropped: &mut usize,
) -> Option<JobId> {
    while let Some(job_id) = queue.front().copied() {
        if jobs.get(job_id).is_some() {
            return Some(job_id);
        }
        queue.pop_front();
        *dropped += 1;
    }
    None
}
