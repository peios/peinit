use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::supervisor::SupervisorInternalErrorDispatch;

use crate::kmes::payload::{
    finish_event, write_optional_str_field, write_optional_string_field, write_str_field,
    write_uint_field,
};

/// An internal error on a per-service path, contained to that service.
///
/// The transition the containment made is `Failed` under `internal_error`
/// like any other, and the job and operation it retired have their own
/// events. This one records what neither of those can: which step peinit
/// could not carry out, and why. Without it the failure would look like a
/// service fault in the audit trail, which is the opposite of what it is
/// (PEI-1125).
pub fn encode_service_internal_error_event(
    dispatch: &SupervisorInternalErrorDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(7);
    write_optional_str_field(&mut writer, "service", dispatch.service());
    write_optional_string_field(
        &mut writer,
        "job_id",
        dispatch.subject.job_id.map(|id| id.to_string()),
    );
    write_str_field(&mut writer, "step", dispatch.step);
    write_str_field(&mut writer, "error", &dispatch.error);
    write_uint_field(&mut writer, "observed_at_ns", dispatch.observed_at_ns);
    write_str_field(
        &mut writer,
        "service_failed",
        if dispatch.service_transition.is_some() {
            "true"
        } else {
            "false"
        },
    );
    write_str_field(&mut writer, "message", &dispatch.message());
    finish_event("service.internal_error", writer)
}

/// What became of an event that could not go into the ring as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OversizedEventAction {
    /// The event was emitted with its `arguments` cut to the budget.
    Truncated,
    /// The ring refused the event and it is gone.
    Dropped,
}

impl OversizedEventAction {
    fn label(self) -> &'static str {
        match self {
            Self::Truncated => "truncated",
            Self::Dropped => "dropped",
        }
    }
}

/// The record of a gap in the audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OversizedEvent<'a> {
    pub event_type: &'a str,
    pub action: OversizedEventAction,
    pub service: Option<&'a str>,
    pub job_id: Option<&'a str>,
    /// The size the event had, or would have had, in payload bytes.
    pub size_bytes: u64,
    /// The argument budget a truncation held the event to.
    pub limit_bytes: Option<u64>,
    /// How many events this boot has dropped, this one included.
    pub dropped_total: Option<u64>,
    pub error: Option<&'a str>,
}

/// An event that was cut or dropped, so the trail records the gap rather
/// than leaving a job with a `job.started` and no `job.ended` (PEI-1082).
///
/// Small by construction: every field is bounded, so this one can always be
/// emitted where the original could not.
pub fn encode_event_oversized_event(
    event: &OversizedEvent<'_>,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(9);
    write_str_field(&mut writer, "event", event.event_type);
    write_str_field(&mut writer, "action", event.action.label());
    write_optional_str_field(&mut writer, "service", event.service);
    write_optional_str_field(&mut writer, "job_id", event.job_id);
    write_uint_field(&mut writer, "size_bytes", event.size_bytes);
    writer.write_str("limit_bytes");
    match event.limit_bytes {
        Some(limit) => {
            writer.write_uint(limit);
        }
        None => {
            writer.write_nil();
        }
    }
    writer.write_str("dropped_total");
    match event.dropped_total {
        Some(total) => {
            writer.write_uint(total);
        }
        None => {
            writer.write_nil();
        }
    }
    write_optional_str_field(&mut writer, "error", event.error);
    write_str_field(&mut writer, "message", &oversized_event_message(event));
    finish_event("event.oversized", writer)
}

/// The `service` and `job_id` an encoded event names, if it names them.
///
/// Every peinit event is a string-keyed map, and the two keys are spelled
/// the same wherever they appear, so the subject of an event the ring
/// refused can be recovered from the payload alone. Anything that does not
/// parse is simply not attributed.
pub fn kmes_event_subject(payload: &[u8]) -> (Option<String>, Option<String>) {
    let mut reader = peios::msgpack::Reader::new(payload);
    let Ok(count) = reader.read_map() else {
        return (None, None);
    };
    let mut service = None;
    let mut job_id = None;
    for _ in 0..count {
        let Ok(key) = reader.read_str() else {
            break;
        };
        let wanted = match key {
            "service" => Some(&mut service),
            "job_id" => Some(&mut job_id),
            _ => None,
        };
        match wanted {
            Some(slot) if reader.peek() == Some(peios::msgpack::Type::Str) => {
                let Ok(value) = reader.read_str() else {
                    break;
                };
                *slot = Some(value.to_string());
            }
            _ => {
                if reader.skip().is_err() {
                    break;
                }
            }
        }
    }
    (service, job_id)
}

pub fn oversized_event_message(event: &OversizedEvent<'_>) -> String {
    let subject = match (event.service, event.job_id) {
        (Some(service), Some(job_id)) => format!(" for service {service} (job {job_id})"),
        (Some(service), None) => format!(" for service {service}"),
        (None, Some(job_id)) => format!(" for job {job_id}"),
        (None, None) => String::new(),
    };
    match event.action {
        OversizedEventAction::Truncated => format!(
            "event {}{subject} had its arguments truncated: {} bytes against a budget of {}",
            event.event_type,
            event.size_bytes,
            event.limit_bytes.unwrap_or(0),
        ),
        OversizedEventAction::Dropped => format!(
            "event {}{subject} ({} bytes) was refused by the event ring and dropped: {}",
            event.event_type,
            event.size_bytes,
            event.error.unwrap_or("unknown error"),
        ),
    }
}
