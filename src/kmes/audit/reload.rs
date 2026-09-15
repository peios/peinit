use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::control::reload_config::{ReloadConfigError, ReloadConfigOutcome};
use crate::supervisor::DeferredRegistryReload;

use crate::kmes::payload::{
    finish_event, write_optional_str_field, write_str_field, write_string_array_field,
    write_uint_field,
};

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

/// A reload during the boot window left definitions pending on boot-plan
/// members whose launch had not been attempted: a boot executes against its
/// snapshot (§3.7), so those members start from the plan and take the
/// change once the window has closed (PEI-350). `events` is the number of
/// definitions deferred by this reload.
pub fn encode_registry_reload_deferred_event(
    services: &[String],
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(3);
    write_uint_field(&mut writer, "events", services.len() as u64);
    write_string_array_field(&mut writer, "services", services);
    write_str_field(
        &mut writer,
        "message",
        &format!(
            "registry changed during the boot window; {} definition(s) deferred until the boot plan drains: {}",
            services.len(),
            services.join(", ")
        ),
    );
    finish_event("config.reload_deferred", writer)
}

/// The one reload that followed the boot window, applying what the reloads
/// during it deferred. This is the mark that the boot's snapshot has been
/// let go of: after it, the registry as it now stands is the running
/// configuration (PEI-350).
pub fn encode_registry_reload_coalesced_event(
    deferred: &DeferredRegistryReload,
    outcome: &Result<ReloadConfigOutcome, ReloadConfigError>,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(10);
    write_uint_field(&mut writer, "deferred", deferred.services.len() as u64);
    write_string_array_field(&mut writer, "services", &deferred.services);
    match outcome {
        Ok(outcome) => {
            write_str_field(&mut writer, "result", "ok");
            write_optional_str_field(&mut writer, "error", None);
            write_uint_field(&mut writer, "added", outcome.summary.added.len() as u64);
            write_uint_field(&mut writer, "updated", outcome.summary.updated.len() as u64);
            write_uint_field(
                &mut writer,
                "restored",
                outcome.summary.restored.len() as u64,
            );
            write_uint_field(
                &mut writer,
                "marked_removed",
                outcome.summary.marked_removed.len() as u64,
            );
            write_uint_field(
                &mut writer,
                "discarded",
                outcome.summary.discarded.len() as u64,
            );
            write_uint_field(
                &mut writer,
                "undecodable",
                outcome.summary.undecodable.len() as u64,
            );
        }
        Err(error) => {
            write_str_field(&mut writer, "result", "error");
            write_optional_str_field(&mut writer, "error", Some(&format!("{error:?}")));
            for field in [
                "added",
                "updated",
                "restored",
                "marked_removed",
                "discarded",
                "undecodable",
            ] {
                write_uint_field(&mut writer, field, 0);
            }
        }
    }
    finish_event("config.reload_coalesced", writer)
}
