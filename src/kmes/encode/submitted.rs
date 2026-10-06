use crate::boundary::{BoundaryError, KmesEvent};
use crate::ids::JobId;
use crate::submitted::JobProgressUnit;
use crate::supervisor::SupervisorSubmittedNotifyDispatch;

use super::super::payload::Payload;
use super::super::types::{JOB_OUTPUT_DROPPED, JOB_STATUS_REPORTED};

/// `peinit.job.status.reported`: the latest `STATUS` / `PROGRESS` a
/// submitted job reported, emitted at most once per job per second (PSPU
/// §4.19).
pub fn encode_job_status_event(
    dispatch: &SupervisorSubmittedNotifyDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.job.guid", dispatch.job_id);
    payload.set_sid("object.job.submitter.sid", &dispatch.submitter_sid);
    payload.set_opt("notify.status", dispatch.status_text.as_deref());
    if let Some(progress) = dispatch.progress {
        payload.set("notify.progress.current", progress.current);
        payload.set_opt("notify.progress.total", progress.total);
        payload.set("notify.progress.bounded", progress.bounded);
    }
    payload.set_opt(
        "notify.progress.unit",
        dispatch.progress_unit.map(JobProgressUnit::wire),
    );
    payload.finish(JOB_STATUS_REPORTED)
}

/// `peinit.job.output.dropped`: a submitter's output sink stopped draining
/// and the manager began dropping its copy of the job's lines. Once per job.
pub fn encode_output_dropped_event(job_id: JobId) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.job.guid", job_id);
    payload.finish(JOB_OUTPUT_DROPPED)
}
