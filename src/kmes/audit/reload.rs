use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::control::reload_config::{ReloadConfigError, ReloadConfigOutcome};
use crate::supervisor::DeferredRegistryReload;

use crate::kmes::payload::{
    finish_event, write_optional_str_field, write_str_field, write_uint_field,
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

/// A registry watch batch arrived while the boot plan was still draining,
/// and was held back: a boot executes against its snapshot (§3.7), so the
/// reload the batch asks for runs once the plan has drained (PEI-350).
pub fn encode_registry_reload_deferred_event(
    events: usize,
    overflow: bool,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(3);
    write_uint_field(&mut writer, "events", events as u64);
    write_str_field(
        &mut writer,
        "overflow",
        if overflow { "true" } else { "false" },
    );
    write_str_field(
        &mut writer,
        "message",
        "registry changed during the boot window; configuration reload deferred until the boot plan drains",
    );
    finish_event("config.reload_deferred", writer)
}

/// The one reload that followed the boot window, standing in for every
/// watch batch and `reload-config` request deferred during it. This is the
/// mark that the boot's snapshot has been let go of: after it, the registry
/// as it now stands is the running configuration (PEI-350).
pub fn encode_registry_reload_coalesced_event(
    deferred: &DeferredRegistryReload,
    outcome: &Result<ReloadConfigOutcome, ReloadConfigError>,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(11);
    write_uint_field(&mut writer, "watch_events", deferred.watch_events as u64);
    write_uint_field(
        &mut writer,
        "explicit_requests",
        deferred.explicit_requests as u64,
    );
    write_str_field(
        &mut writer,
        "overflow",
        if deferred.overflow { "true" } else { "false" },
    );
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
