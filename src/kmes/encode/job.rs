use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::job::{JobEvent, JobEventDetail};

use super::super::labels::{job_state_label, job_type_label};
use super::super::payload::{
    finish_event, write_bool_field, write_optional_i32_field, write_optional_str_field,
    write_optional_string_field, write_optional_u32_field, write_optional_u64_field,
    write_str_field, write_string_array_field, write_token_summary_field, write_uint_field,
};

/// The most of a job's `arguments` a `job.ended` carries, in encoded bytes.
///
/// `job.ended` carries the whole record, and the record's `arguments` are
/// bounded only by what admitted them: `MaxJobMessageSize` for a submitted
/// job, the registry for a service. KMES refuses an event over
/// `MaxEventSize` (65536 by default), and an event PID 1 cannot emit must
/// never be PID 1's problem (PEI-1082). So the arguments are cut to this
/// budget — half the default `MaxEventSize`, leaving the other half for the
/// record's other twenty-odd fields — and the event says when they were.
/// The default `MaxJobMessageSize` is held at or below this budget, so a
/// record that fills a default-sized message is never cut: a MessagePack
/// string costs at most three bytes over its length, a JSON one at least
/// two plus the record's framing, so the encoded arguments of a record are
/// always smaller than the record that carried them.
pub const MAX_JOB_ENDED_ARGUMENTS_BYTES: usize = 32 * 1024;

/// What a `job.ended` left out to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobEventTruncation {
    /// The encoded size the arguments would have had.
    pub arguments_bytes: u64,
    /// The budget they were cut to.
    pub limit_bytes: u64,
}

pub fn encode_job_event(event: &JobEvent) -> Result<KmesEvent, BoundaryError> {
    encode_job_event_bounded(event).map(|(event, _)| event)
}

/// Encode the event, and say whether its arguments were cut to fit.
pub fn encode_job_event_bounded(
    event: &JobEvent,
) -> Result<(KmesEvent, Option<JobEventTruncation>), BoundaryError> {
    match &event.detail {
        JobEventDetail::Created {
            image_path,
            identity,
            operation_id,
        } => Ok((
            encode_created_job_event(event, image_path, identity, *operation_id)?,
            None,
        )),
        JobEventDetail::Started {
            started_at_ns,
            pid,
            cgroup_id,
        } => Ok((
            encode_started_job_event(event, *started_at_ns, *pid, cgroup_id)?,
            None,
        )),
        JobEventDetail::Ended {
            ended_at_ns,
            duration_ns,
            exit_code,
            exit_signal,
            failure_cause,
        } => encode_ended_job_event(
            event,
            *ended_at_ns,
            *duration_ns,
            *exit_code,
            *exit_signal,
            failure_cause.as_deref(),
        ),
    }
}

fn encode_created_job_event(
    event: &JobEvent,
    image_path: &str,
    identity: &str,
    operation_id: Option<crate::ids::OperationId>,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(11);
    write_job_common(&mut writer, event);
    write_str_field(&mut writer, "image_path", image_path);
    write_str_field(&mut writer, "identity", identity);
    write_optional_string_field(
        &mut writer,
        "created_operation_id",
        operation_id.map(|id| id.to_string()),
    );
    finish_event("job.created", writer)
}

fn encode_started_job_event(
    event: &JobEvent,
    started_at_ns: u64,
    pid: u32,
    cgroup_id: &str,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(11);
    write_job_common(&mut writer, event);
    write_uint_field(&mut writer, "started_at_ns", started_at_ns);
    write_uint_field(&mut writer, "pid", u64::from(pid));
    write_str_field(&mut writer, "cgroup_id", cgroup_id);
    finish_event("job.started", writer)
}

fn encode_ended_job_event(
    event: &JobEvent,
    ended_at_ns: u64,
    duration_ns: u64,
    exit_code: Option<i32>,
    exit_signal: Option<i32>,
    failure_cause: Option<&str>,
) -> Result<(KmesEvent, Option<JobEventTruncation>), BoundaryError> {
    let (arguments, truncation) = bounded_arguments(&event.arguments);
    let mut writer = Writer::new();
    writer.write_map(24);
    write_job_common(&mut writer, event);
    write_optional_u32_field(&mut writer, "pid", event.pid);
    write_optional_i32_field(&mut writer, "pidfd", event.pidfd);
    write_str_field(&mut writer, "resolved_identity", &event.resolved_identity);
    write_str_field(&mut writer, "image_path", &event.image_path);
    write_string_array_field(&mut writer, "arguments", arguments);
    write_bool_field(&mut writer, "arguments_truncated", truncation.is_some());
    write_uint_field(
        &mut writer,
        "arguments_total",
        u64::try_from(event.arguments.len()).unwrap_or(u64::MAX),
    );
    write_uint_field(&mut writer, "created_at_ns", event.created_at_ns);
    write_optional_u64_field(&mut writer, "started_at_ns", event.started_at_ns);
    write_uint_field(&mut writer, "ended_at_ns", ended_at_ns);
    write_uint_field(&mut writer, "duration_ns", duration_ns);
    write_optional_i32_field(&mut writer, "exit_code", exit_code);
    write_optional_i32_field(&mut writer, "exit_signal", exit_signal);
    write_optional_str_field(&mut writer, "failure_cause", failure_cause);
    write_str_field(&mut writer, "cgroup_id", &event.cgroup_id);
    write_uint_field(&mut writer, "cgroup_generation", event.cgroup_generation);
    Ok((finish_event("job.ended", writer)?, truncation))
}

/// The longest prefix of `arguments` whose encoding fits the budget, and
/// what was cut if the whole did not.
///
/// Whole arguments only: a cut argument would read as a different command
/// line, and the count of what is missing is on the event beside them.
fn bounded_arguments(arguments: &[String]) -> (&[String], Option<JobEventTruncation>) {
    let total: usize = arguments
        .iter()
        .map(|argument| msgpack_str_size(argument.len()))
        .sum();
    if total <= MAX_JOB_ENDED_ARGUMENTS_BYTES {
        return (arguments, None);
    }
    let mut used = 0;
    let mut kept = 0;
    for argument in arguments {
        let size = msgpack_str_size(argument.len());
        if used + size > MAX_JOB_ENDED_ARGUMENTS_BYTES {
            break;
        }
        used += size;
        kept += 1;
    }
    (
        &arguments[..kept],
        Some(JobEventTruncation {
            arguments_bytes: total as u64,
            limit_bytes: MAX_JOB_ENDED_ARGUMENTS_BYTES as u64,
        }),
    )
}

/// The encoded size of a MessagePack string of `len` bytes: the header the
/// writer picks for that length, plus the bytes.
fn msgpack_str_size(len: usize) -> usize {
    let header = match len {
        0..=31 => 1,
        32..=255 => 2,
        256..=65_535 => 3,
        _ => 5,
    };
    header + len
}

fn write_job_common(writer: &mut Writer, event: &JobEvent) {
    write_str_field(writer, "job_id", &event.job_id.to_string());
    write_optional_str_field(writer, "service", event.service.as_deref());
    write_str_field(writer, "type", job_type_label(event.job_type));
    write_str_field(writer, "state", job_state_label(event.state));
    write_optional_string_field(
        writer,
        "operation_id",
        event.operation_id.map(|id| id.to_string()),
    );
    write_token_summary_field(writer, "token_summary", &event.token_summary);
    write_str_field(writer, "token_identity", &event.token_summary.identity);
    write_str_field(writer, "final_state", job_state_label(event.state));
}
