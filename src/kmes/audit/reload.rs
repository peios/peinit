use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};

use crate::kmes::payload::{finish_event, write_str_field};

/// A service announced `RELOADING=1` and then never completed the reload.
///
/// This is what the detection protocol exists to catch, and it used to leave
/// no record anywhere. The warning text existed only as an *operation result*,
/// returned to a `wait=true` caller — and `reload` defaults to `wait=false`,
/// so the default way to issue one produced nothing at all when the service
/// wedged mid-reload or lost its handler (PEI-359).
///
/// An advisory outcome after an explicit `RELOADING=1` is a real diagnostic,
/// not a routine result, so it is audited like `on_failure.loop_suppressed`
/// and `graph.validation_warning`.
pub fn encode_reload_unconfirmed_event(service: &str) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(2);
    write_str_field(&mut writer, "service", service);
    write_str_field(
        &mut writer,
        "message",
        &format!("service {service} signalled RELOADING=1 but never completed reload"),
    );
    finish_event("service.reload_unconfirmed", writer)
}
