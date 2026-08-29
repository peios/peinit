use crate::boundary::{ProcessController, ProcessSignal, ProcessTarget};
use crate::ids::JobId;
use crate::job::JobState;
use crate::submitted::SubmittedJobCause;

use super::setup::apply_submitted_setup_failure;
use crate::supervisor::dispatch::SupervisorSubmittedStopDispatch;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

/// What a stop of a submitted job did.
pub(in crate::supervisor) enum SubmittedStopOutcome {
    /// Already terminal or already stopping: nothing changed.
    Unchanged,
    /// The job never ran; it is now terminal.
    CancelledBeforeStart(
        Box<crate::supervisor::dispatch::SupervisorSubmittedLaunchFailureDispatch>,
    ),
    /// A stop is in progress.
    Stopping(SupervisorSubmittedStopDispatch),
}

/// Begin stopping a submitted job for `cause` (PSPU §7.8): the termination
/// signal now — unless the job said it was already stopping — and the kill
/// at `stop_timeout`. A stop already in progress is not restarted.
pub(in crate::supervisor) fn begin_submitted_stop<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    job_id: JobId,
    cause: SubmittedJobCause,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<SubmittedStopOutcome, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let Some(entry) = work.submitted.get(job_id) else {
        return Ok(SubmittedStopOutcome::Unchanged);
    };
    if entry.outcome.is_some() || entry.stop.is_some() {
        return Ok(SubmittedStopOutcome::Unchanged);
    }
    let stopping_acknowledged = entry.stopping_acknowledged;
    let Some(record) = work.jobs.get(job_id).cloned() else {
        return Ok(SubmittedStopOutcome::Unchanged);
    };

    if record.state == JobState::Created {
        // Queued but not launched: it never runs. Pending exec is handled by
        // the setup completion, which sees the stop it is about to record.
        if let Some(position) = work
            .pending_submitted_launches
            .iter()
            .position(|pending| *pending == job_id)
        {
            work.pending_submitted_launches.remove(position);
            let failure = apply_submitted_setup_failure(
                work,
                controller,
                job_id,
                now_ns,
                cause,
                format!("{}: stopped before launch", cause.wire()),
                post_kill_timeout_secs,
            )?;
            return Ok(SubmittedStopOutcome::CancelledBeforeStart(Box::new(
                failure,
            )));
        }
        work.submitted
            .begin_stop(job_id, cause, now_ns)
            .map_err(SupervisorError::Submitted)?;
        controller
            .kill_cgroup(&record.cgroup_id)
            .map_err(SupervisorError::ProcessControl)?;
        return Ok(SubmittedStopOutcome::Stopping(
            SupervisorSubmittedStopDispatch {
                job_id,
                cause,
                signalled: false,
            },
        ));
    }

    work.submitted
        .begin_stop(job_id, cause, now_ns)
        .map_err(SupervisorError::Submitted)?;
    let signalled = !stopping_acknowledged;
    if signalled {
        let target = submitted_process_target(&record)?;
        controller
            .signal_main(&target, ProcessSignal::Sigterm)
            .map_err(SupervisorError::ProcessControl)?;
    }
    Ok(SubmittedStopOutcome::Stopping(
        SupervisorSubmittedStopDispatch {
            job_id,
            cause,
            signalled,
        },
    ))
}

/// The signal target for a running submitted job. `service` carries the job
/// identifier, since the boundary's target model names its subject that way.
pub(in crate::supervisor) fn submitted_process_target(
    record: &crate::job::JobRecord,
) -> Result<ProcessTarget, SupervisorError> {
    let (Some(pid), Some(pidfd)) = (record.pid, record.pidfd) else {
        return Err(SupervisorError::Submitted(
            crate::submitted::SubmittedJobStoreError::NotTerminal { id: record.id },
        ));
    };
    Ok(ProcessTarget {
        service: record.id.to_canonical_string(),
        pid,
        pidfd,
        cgroup_id: record.cgroup_id.clone(),
    })
}
