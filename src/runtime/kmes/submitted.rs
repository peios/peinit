//! Event collection for submitted jobs: their lifecycle events ride the
//! ordinary job encoders; their status, output drops and leaked cgroups
//! have encoders of their own. A job command refused by the job's
//! descriptor writes no event of peinit's: KACS records the decision
//! (PEI-1279).

use crate::boundary::BoundaryError;
use crate::kmes::types::{CGROUP_LEAKED, JOB_OUTPUT_DROPPED, JOB_STATUS_REPORTED};
use crate::kmes::{
    EventCollector, encode_job_status_event, encode_leaked_job_cgroup_event,
    encode_output_dropped_event,
};
use crate::runtime::RuntimeJobsConnectionTurn;
use crate::supervisor::{
    SupervisedSubmittedTerminalDispatch, SupervisorJobsCommandDispatch,
    SupervisorSubmittedDeadlineDispatch, SupervisorSubmittedLaunchDispatch,
    SupervisorSubmittedLaunchFailureDispatch, SupervisorSubmittedNotifyDispatch,
};

use super::event::push_job;

pub(super) fn collect_submitted_launch(
    dispatch: &SupervisorSubmittedLaunchDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.launch.job_event)
}

pub(super) fn collect_submitted_launch_failure(
    dispatch: &SupervisorSubmittedLaunchFailureDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)
}

pub(super) fn collect_submitted_terminal(
    dispatch: &SupervisedSubmittedTerminalDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)
}

pub(super) fn collect_jobs_command_dispatch(
    dispatch: &SupervisorJobsCommandDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    match dispatch {
        SupervisorJobsCommandDispatch::Submit(dispatch) => push_job(out, &dispatch.job_event),
        SupervisorJobsCommandDispatch::Cancelled(dispatch) => push_job(out, &dispatch.job_event),
        SupervisorJobsCommandDispatch::Stop(_) | SupervisorJobsCommandDispatch::Signal { .. } => {
            Ok(())
        }
    }
}

pub(super) fn collect_jobs_connection_turn(
    turn: &RuntimeJobsConnectionTurn,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    let RuntimeJobsConnectionTurn::Processed { supervisor, .. } = turn else {
        return Ok(());
    };
    if let Some(dispatch) = &supervisor.dispatch {
        collect_jobs_command_dispatch(dispatch, out)?;
    }
    Ok(())
}

pub(super) fn collect_submitted_deadline(
    dispatch: &SupervisorSubmittedDeadlineDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    match dispatch {
        SupervisorSubmittedDeadlineDispatch::Abandoned {
            job_event,
            cgroup_id,
        } => {
            push_job(out, job_event)?;
            out.push(CGROUP_LEAKED, |_| {
                encode_leaked_job_cgroup_event(job_event.job_id, cgroup_id)
            })
        }
        SupervisorSubmittedDeadlineDispatch::CgroupCleanup {
            job_id,
            leaked: true,
        } => out.push(CGROUP_LEAKED, |_| {
            encode_leaked_job_cgroup_event(*job_id, &crate::job::submitted_job_cgroup_path(*job_id))
        }),
        SupervisorSubmittedDeadlineDispatch::Stop(_)
        | SupervisorSubmittedDeadlineDispatch::Killed { .. }
        | SupervisorSubmittedDeadlineDispatch::CgroupCleanup { .. } => Ok(()),
    }
}

pub(super) fn collect_submitted_notify(
    dispatch: &SupervisorSubmittedNotifyDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    if !dispatch.status_event_due {
        return Ok(());
    }
    out.push(JOB_STATUS_REPORTED, |_| encode_job_status_event(dispatch))
}

pub(super) fn collect_output_dropped(
    job_id: crate::ids::JobId,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    out.push(JOB_OUTPUT_DROPPED, |_| encode_output_dropped_event(job_id))
}
