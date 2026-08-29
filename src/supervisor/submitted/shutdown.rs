//! Submitted jobs during a system shutdown (PSPU §7.10, Peinit TRM §12):
//! every live job is stopped when the shutdown begins, with no ordering;
//! the shutdown is not finished while one is live; the global timeout
//! kills whatever is left.

use crate::boundary::ProcessController;
use crate::submitted::SubmittedJobCause;

use super::stop::{SubmittedStopOutcome, begin_submitted_stop};
use crate::supervisor::dispatch::SupervisorSubmittedStopDispatch;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

pub(in crate::supervisor) fn stop_submitted_jobs_for_shutdown<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<Vec<SupervisorSubmittedStopDispatch>, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let mut stops = Vec::new();
    for job_id in work.submitted.live_ids() {
        if let SubmittedStopOutcome::Stopping(stop) = begin_submitted_stop(
            work,
            controller,
            job_id,
            SubmittedJobCause::Shutdown,
            now_ns,
            post_kill_timeout_secs,
        )? {
            stops.push(stop);
        }
    }
    Ok(stops)
}

impl crate::supervisor::state::Supervisor {
    /// A submitted job ended during a shutdown: it may have been the last
    /// thing the shutdown was waiting for.
    pub(in crate::supervisor) fn advance_shutdown_after_submitted_job<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<(), SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        crate::supervisor::shutdown_progress::advance_shutdown_progress(
            &mut work, controller, now_ns,
        )
        .map_err(SupervisorError::Shutdown)?;
        work.commit(self);
        Ok(())
    }
}

pub(in crate::supervisor) fn live_submitted_jobs_remain(work: &SupervisorWork) -> bool {
    !work.submitted.live_ids().is_empty()
}

/// The global timeout: kill every live job's cgroup and arm its post-kill
/// check, whatever phase its stop was in.
pub(in crate::supervisor) fn kill_all_live_submitted_jobs<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<Vec<crate::ids::JobId>, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let mut killed = Vec::new();
    for job_id in work.submitted.live_ids() {
        let Some(cgroup_id) = work
            .submitted
            .get(job_id)
            .map(|entry| entry.cgroup_id.clone())
        else {
            continue;
        };
        if work
            .submitted
            .get(job_id)
            .is_some_and(|entry| entry.stop.is_none())
        {
            work.submitted
                .begin_stop(job_id, SubmittedJobCause::Shutdown, now_ns)
                .map_err(SupervisorError::Submitted)?;
        }
        controller
            .kill_cgroup(&cgroup_id)
            .map_err(SupervisorError::ProcessControl)?;
        work.submitted
            .record_kill(job_id, now_ns, post_kill_timeout_secs)
            .map_err(SupervisorError::Submitted)?;
        killed.push(job_id);
    }
    Ok(killed)
}
