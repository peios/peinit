use peios::msgpack::{Reader, Type};

use crate::boundary::{BoundaryError, KmesEvent};
use crate::supervisor::SupervisorInternalErrorDispatch;

use crate::kmes::payload::{Payload, Value};
use crate::kmes::types::{EVENT_DROPPED, INTERNAL_ERROR_CONTAINED};

/// `peinit.internal-error.contained`: an internal error on a per-service
/// path, contained to that service.
///
/// The transition the containment made is `failed` under `internal-error`
/// like any other, and the job and operation it retired have their own
/// events. This one records what neither of those can: which step peinit
/// could not carry out. Without it the failure would look like a service
/// fault in the audit trail, which is the opposite of what it is
/// (PEI-1125).
///
/// The error itself is not carried. Every path that contains one renders it
/// as Rust Debug output, which an event never carries (PGSS §6.5); the
/// console line and the failed operation's result keep it.
pub fn encode_service_internal_error_event(
    dispatch: &SupervisorInternalErrorDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("operation.stage", stage_label(dispatch.step));
    if let Some(service) = dispatch.service() {
        payload.set("object.service.name", service);
        payload.set("object.service.failed", dispatch.service_transition.is_some());
    }
    payload.set_opt("object.job.guid", dispatch.subject.job_id);
    payload.finish(INTERNAL_ERROR_CONTAINED)
}

/// The console's words for a step (`job terminal`), as an event value.
fn stage_label(step: &str) -> String {
    step.replace(' ', "-")
}

/// An event the ring refused, as `peinit.event.dropped` records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedEvent<'a> {
    pub event_type: &'a str,
    pub subject: &'a KmesEventSubject,
    /// The refused payload's length in bytes.
    pub payload_length: u64,
    /// The error number the emit returned, positive as the kernel gave it.
    pub errno: Option<i32>,
}

/// `peinit.event.dropped`: the ring refused one of peinit's events, and it
/// is gone.
///
/// Written so the trail records the gap rather than, say, a job with a
/// `peinit.job.started` and no `peinit.job.ended` (PEI-1082). Small by
/// construction: every field is bounded, so this one can always be emitted
/// where the original could not.
pub fn encode_event_dropped_event(event: &DroppedEvent<'_>) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("emission.type", event.event_type);
    payload.set("emission.payload-length", event.payload_length);
    payload.set_opt("object.service.name", event.subject.service.as_deref());
    payload.set_opt(
        "object.job.guid",
        event.subject.job_guid.map(|guid| Value::Bin(guid.to_vec())),
    );
    payload.set_opt(
        "outcome.errno",
        event
            .errno
            .filter(|errno| *errno > 0)
            .map(|errno| -i64::from(errno)),
    );
    payload.finish(EVENT_DROPPED)
}

/// The service and job an encoded event is about, if it names them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KmesEventSubject {
    pub service: Option<String>,
    pub job_guid: Option<[u8; 16]>,
}

impl KmesEventSubject {
    /// The job's GUID as its canonical text, for a person: the PCDS binary
    /// form read back, first three fields little-endian (PCDS §2.2).
    pub fn job_text(&self) -> Option<String> {
        self.job_guid.map(|b| {
            format!(
                "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                u16::from_le_bytes([b[4], b[5]]),
                u16::from_le_bytes([b[6], b[7]]),
                b[8],
                b[9],
                b[10],
                b[11],
                b[12],
                b[13],
                b[14],
                b[15]
            )
        })
    }
}

/// The service and job an encoded event names, if it names them.
///
/// Every peinit event is a nested string-keyed map, and a service or a job
/// is named at one of a few fixed paths: `object.service.name` and
/// `object.job.guid` when the event is about it, `subject.service.name` and
/// `subject.job.guid` when it is the sender of a notification. So the
/// subject of an event the ring refused can be recovered from the payload
/// alone. Anything that does not parse is simply not attributed.
pub fn kmes_event_subject(payload: &[u8]) -> KmesEventSubject {
    let service = ["object", "subject"]
        .iter()
        .find_map(|root| find_str(payload, &[root, "service", "name"]));
    let job_guid = ["object", "subject"]
        .iter()
        .find_map(|root| find_guid(payload, &[root, "job", "guid"]));
    KmesEventSubject { service, job_guid }
}

/// A reader positioned at the value at `path` through nested maps.
fn seek<'a>(payload: &'a [u8], path: &[&str]) -> Option<Reader<'a>> {
    let mut reader = Reader::new(payload);
    'segments: for segment in path {
        if reader.peek() != Some(Type::Map) {
            return None;
        }
        let count = reader.read_map().ok()?;
        for _ in 0..count {
            let key = reader.read_str().ok()?;
            if key == *segment {
                continue 'segments;
            }
            reader.skip().ok()?;
        }
        return None;
    }
    Some(reader)
}

fn find_str(payload: &[u8], path: &[&str]) -> Option<String> {
    let mut reader = seek(payload, path)?;
    if reader.peek() != Some(Type::Str) {
        return None;
    }
    reader.read_str().ok().map(str::to_string)
}

fn find_guid(payload: &[u8], path: &[&str]) -> Option<[u8; 16]> {
    let mut reader = seek(payload, path)?;
    if reader.peek() != Some(Type::Bin) {
        return None;
    }
    reader.read_bin().ok()?.try_into().ok()
}

/// The console's account of a dropped event, for a person.
pub fn dropped_event_message(event: &DroppedEvent<'_>, error: &str) -> String {
    let subject = match (event.subject.service.as_deref(), event.subject.job_text()) {
        (Some(service), Some(job)) => format!(" for service {service} (job {job})"),
        (Some(service), None) => format!(" for service {service}"),
        (None, Some(job)) => format!(" for job {job}"),
        (None, None) => String::new(),
    };
    format!(
        "event {}{subject} ({} bytes) was refused by the event ring and dropped: {error}",
        event.event_type, event.payload_length,
    )
}
