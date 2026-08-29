use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::ids::JobId;
use crate::submitted::{JobAccessDenied, JobProgressUnit};
use crate::supervisor::SupervisorSubmittedNotifyDispatch;

use super::super::payload::{
    finish_event, write_optional_str_field, write_optional_u64_field, write_str_field,
    write_uint_field,
};

/// `job.status` — the latest `STATUS` / `PROGRESS` a submitted job reported,
/// emitted at most once per job per second (PSPU §4.19).
pub fn encode_job_status_event(
    dispatch: &SupervisorSubmittedNotifyDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let status_text = dispatch.status_text.as_deref();
    let progress = dispatch.progress;
    let unit = dispatch.progress_unit;
    let mut writer = Writer::new();
    writer.write_map(7);
    write_str_field(&mut writer, "job_id", &dispatch.job_id.to_string());
    write_str_field(&mut writer, "submitter", &dispatch.submitter_sid);
    write_optional_str_field(&mut writer, "status", status_text);
    write_optional_u64_field(&mut writer, "progress_current", progress.map(|p| p.current));
    write_optional_u64_field(
        &mut writer,
        "progress_total",
        progress.and_then(|p| p.total),
    );
    writer.write_str("progress_bounded");
    match progress {
        Some(progress) => {
            writer.write_bool(progress.bounded);
        }
        None => {
            writer.write_nil();
        }
    }
    write_optional_str_field(
        &mut writer,
        "progress_unit",
        unit.map(JobProgressUnit::wire),
    );
    finish_event("job.status", writer)
}

/// `output.dropped` — a submitter's output sink stopped draining and the
/// manager began dropping its copy of the job's lines. Once per job.
pub fn encode_output_dropped_event(job_id: JobId) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(2);
    write_str_field(&mut writer, "job_id", &job_id.to_string());
    write_str_field(
        &mut writer,
        "message",
        "the submitter's output sink would block; lines are dropped for the sink only",
    );
    finish_event("output.dropped", writer)
}

/// `job.access_denied` — a job command refused by the job's descriptor.
pub fn encode_job_access_denied_event(
    denied: &JobAccessDenied,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(6);
    write_str_field(&mut writer, "caller_sid", denied.caller.caller_sid());
    write_str_field(&mut writer, "target_type", "job");
    write_str_field(&mut writer, "target", &denied.job_id.to_string());
    write_str_field(
        &mut writer,
        "requested_right",
        denied.desired_access.label(),
    );
    write_uint_field(
        &mut writer,
        "requested_access_bits",
        u64::from(denied.desired_access.bits()),
    );
    write_uint_field(
        &mut writer,
        "granted_access_bits",
        u64::from(denied.granted_access_bits),
    );
    finish_event("job.access_denied", writer)
}
