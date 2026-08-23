use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::service::runtime::LeakedCgroupKind;
use crate::supervisor::SupervisorLeakedCgroupDispatch;

use crate::kmes::payload::{finish_event, write_str_field, write_uint_field};

pub fn encode_leaked_cgroup_event(
    event: &SupervisorLeakedCgroupDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(5);
    write_str_field(&mut writer, "service", &event.service);
    write_str_field(&mut writer, "path", &event.path);
    write_str_field(&mut writer, "type", leaked_cgroup_kind(event.kind));
    write_uint_field(&mut writer, "detected_at_ns", event.detected_at_ns);
    write_str_field(
        &mut writer,
        "message",
        &format!(
            "{} sub-cgroup {} for service {} could not be reclaimed and is leaked -- underlying process is not responding to the kernel",
            leaked_cgroup_kind(event.kind),
            event.path,
            event.service,
        ),
    );
    finish_event("cgroup.leaked", writer)
}

/// The same spellings a `status` query's `warnings` array uses, so an operator
/// reading the event stream and an operator polling `status` see one vocabulary.
fn leaked_cgroup_kind(kind: LeakedCgroupKind) -> &'static str {
    match kind {
        LeakedCgroupKind::ServiceTree => "service_tree",
        LeakedCgroupKind::Health => "health",
        LeakedCgroupKind::Hooks => "hooks",
        LeakedCgroupKind::Helper => "helper",
    }
}
