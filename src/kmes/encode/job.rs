use crate::boundary::{BoundaryError, EventTimeProjection, KmesEvent};
use crate::job::{JobEvent, JobEventDetail, JobState, JobType};

use super::super::labels::{job_state_label, job_type_label};
use super::super::payload::{JOB_TOKEN, Payload};
use super::super::types::{JOB_CREATED, JOB_ENDED, JOB_STARTED};

/// The most of a job's `object.job.arguments` a `peinit.job.ended`
/// carries, in encoded bytes.
///
/// `peinit.job.ended` carries the whole record, and the record's arguments
/// are bounded only by what admitted them: `MaxJobMessageSize` for a
/// submitted job, the registry for a service. KMES refuses an event over
/// `MaxEventSize` (65536 by default), and an event PID 1 cannot emit must
/// never be PID 1's problem (PEI-1082). So the arguments are cut to this
/// budget — half the default `MaxEventSize`, leaving the other half for the
/// record's other fields — and `object.job.arguments-truncated` says when
/// they were. The default `MaxJobMessageSize` is held at or below this
/// budget, so a record that fills a default-sized message is never cut: a
/// MessagePack string costs at most three bytes over its length, a JSON one
/// at least two plus the record's framing, so the encoded arguments of a
/// record are always smaller than the record that carried them.
pub const MAX_JOB_ENDED_ARGUMENTS_BYTES: usize = 32 * 1024;

/// The type a job event is written as.
pub fn job_event_type(event: &JobEvent) -> &'static str {
    match event.detail {
        JobEventDetail::Created { .. } => JOB_CREATED,
        JobEventDetail::Started { .. } => JOB_STARTED,
        JobEventDetail::Ended { .. } => JOB_ENDED,
    }
}

/// `peinit.job.created`, `.started` or `.ended`, as the event's detail says.
pub fn encode_job_event(
    event: &JobEvent,
    time: EventTimeProjection,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = job_common(event);
    match &event.detail {
        JobEventDetail::Created {
            image_path,
            operation_id,
            ..
        } => {
            // The operation the record was created for; the record's own
            // `operation_id` is the same one, set when it exists.
            payload.set_opt("object.operation.guid", operation_id.or(event.operation_id));
            payload.set("object.job.executable", image_path.as_str());
            payload.finish(JOB_CREATED)
        }
        JobEventDetail::Started { pid, cgroup_id, .. } => {
            payload.set("object.process.pid", *pid);
            payload.set("object.cgroup.path", cgroup_id.as_str());
            payload.finish(JOB_STARTED)
        }
        JobEventDetail::Ended {
            duration_ns,
            exit_code,
            exit_signal,
            failure_cause,
            ..
        } => {
            let completed = event.state == JobState::Completed;
            payload.set("outcome.success", completed);
            if !completed {
                payload.set_opt("outcome.detail", failure_cause.as_deref());
            }
            payload.set_opt("object.process.pid", event.pid);
            payload.set_opt("object.process.exit-code", *exit_code);
            payload.set_opt("object.process.exit-signal", *exit_signal);
            payload.set("object.job.executable", event.image_path.as_str());
            let (arguments, truncated) = bounded_arguments(&event.arguments);
            payload.set("object.job.arguments", arguments);
            payload.set("object.job.arguments-truncated", truncated);
            payload.set(
                "object.job.arguments-count",
                u64::try_from(event.arguments.len()).unwrap_or(u64::MAX),
            );
            payload.set("object.job.created-time", time.realtime_ns(event.created_at_ns));
            payload.set_opt(
                "object.job.started-time",
                event.started_at_ns.map(|ns| time.realtime_ns(ns)),
            );
            payload.set("object.job.duration", *duration_ns);
            payload.set("object.cgroup.path", event.cgroup_id.as_str());
            payload.set("object.cgroup.generation", event.cgroup_generation);
            payload.finish(JOB_ENDED)
        }
    }
}

/// What every job event carries: the job, whose it is, and the token its
/// process runs as.
fn job_common(event: &JobEvent) -> Payload {
    let mut payload = Payload::new();
    payload.set("object.job.guid", event.job_id);
    payload.set("object.job.type", job_type_label(event.job_type));
    payload.set("object.job.state", job_state_label(event.state));
    if event.job_type != JobType::Submitted {
        payload.set_opt("object.service.name", event.service.as_deref());
        payload.set("object.job.activation-generation", event.activation_generation);
    }
    payload.set_opt("object.operation.guid", event.operation_id);
    payload.set_token(JOB_TOKEN, &event.token_summary);
    payload
}

/// The longest prefix of `arguments` whose encoding fits the budget, and
/// whether that is less than the whole.
///
/// Whole arguments only: a cut argument would read as a different command
/// line, and the count of what is missing is on the event beside them.
fn bounded_arguments(arguments: &[String]) -> (&[String], bool) {
    let total: usize = arguments
        .iter()
        .map(|argument| msgpack_str_size(argument.len()))
        .sum();
    if total <= MAX_JOB_ENDED_ARGUMENTS_BYTES {
        return (arguments, false);
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
    (&arguments[..kept], true)
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
