//! Event collection for submitted jobs: their lifecycle events ride the
//! ordinary job encoders; their status, output drops, access denials and
//! leaked cgroups have encoders of their own.

use crate::boundary::{BoundaryError, KmesEvent};
use crate::kmes::{
    encode_job_access_denied_event, encode_job_status_event, encode_leaked_cgroup_event,
    encode_output_dropped_event,
};
use crate::runtime::RuntimeJobsConnectionTurn;
use crate::service::runtime::LeakedCgroupKind;
use crate::supervisor::{
    SupervisedSubmittedTerminalDispatch, SupervisorJobsCommandDispatch,
    SupervisorLeakedCgroupDispatch, SupervisorSubmittedDeadlineDispatch,
    SupervisorSubmittedLaunchDispatch, SupervisorSubmittedLaunchFailureDispatch,
    SupervisorSubmittedNotifyDispatch,
};

use super::event::push_job;

pub(super) fn collect_submitted_launch(
    dispatch: &SupervisorSubmittedLaunchDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.launch.job_event)
}

pub(super) fn collect_submitted_launch_failure(
    dispatch: &SupervisorSubmittedLaunchFailureDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)
}

pub(super) fn collect_submitted_terminal(
    dispatch: &SupervisedSubmittedTerminalDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)
}

pub(super) fn collect_jobs_command_dispatch(
    dispatch: &SupervisorJobsCommandDispatch,
    out: &mut Vec<KmesEvent>,
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
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    let RuntimeJobsConnectionTurn::Processed { supervisor, .. } = turn else {
        return Ok(());
    };
    if let Some(denied) = &supervisor.access_denial {
        out.push(encode_job_access_denied_event(denied)?);
    }
    if let Some(dispatch) = &supervisor.dispatch {
        collect_jobs_command_dispatch(dispatch, out)?;
    }
    Ok(())
}

pub(super) fn collect_submitted_deadline(
    dispatch: &SupervisorSubmittedDeadlineDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match dispatch {
        SupervisorSubmittedDeadlineDispatch::Abandoned {
            job_event,
            cgroup_id,
        } => {
            push_job(out, job_event)?;
            out.push(encode_leaked_cgroup_event(
                &SupervisorLeakedCgroupDispatch {
                    service: format!("jobs/{}", job_event.job_id),
                    path: cgroup_id.clone(),
                    kind: LeakedCgroupKind::ServiceTree,
                    detected_at_ns: job_event.ended_at_ns.unwrap_or(0),
                },
            )?);
            Ok(())
        }
        SupervisorSubmittedDeadlineDispatch::CgroupCleanup {
            job_id,
            leaked: true,
        } => {
            out.push(encode_leaked_cgroup_event(
                &SupervisorLeakedCgroupDispatch {
                    service: format!("jobs/{job_id}"),
                    path: crate::job::submitted_job_cgroup_path(*job_id),
                    kind: LeakedCgroupKind::ServiceTree,
                    detected_at_ns: 0,
                },
            )?);
            Ok(())
        }
        SupervisorSubmittedDeadlineDispatch::Stop(_)
        | SupervisorSubmittedDeadlineDispatch::Killed { .. }
        | SupervisorSubmittedDeadlineDispatch::CgroupCleanup { .. } => Ok(()),
    }
}

pub(super) fn collect_submitted_notify(
    dispatch: &SupervisorSubmittedNotifyDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    if !dispatch.status_event_due {
        return Ok(());
    }
    out.push(encode_job_status_event(dispatch)?);
    Ok(())
}

pub(super) fn collect_output_dropped(
    job_id: crate::ids::JobId,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    out.push(encode_output_dropped_event(job_id)?);
    Ok(())
}
