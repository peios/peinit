use crate::boundary::{BoundaryError, KmesEvent};
use crate::control::reload_config::{ReloadConfigError, ReloadConfigOutcome};

use crate::kmes::payload::Payload;
use crate::kmes::types::{CONFIG_RELOAD_APPLIED, CONFIG_RELOAD_DEFERRED, SERVICE_RELOAD_TIMED_OUT};

/// `peinit.service.reload.timed-out`: a service announced `RELOADING=1` and
/// then never completed the reload.
///
/// This is what the detection protocol exists to catch, and it used to leave
/// no record anywhere. The warning text existed only as an *operation
/// result*, returned to a `wait=true` caller — and `reload` defaults to
/// `wait=false`, so the default way to issue one produced nothing at all when
/// the service wedged mid-reload or lost its handler (PEI-359).
pub fn encode_reload_unconfirmed_event(service: &str) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.service.name", service);
    payload.finish(SERVICE_RELOAD_TIMED_OUT)
}

/// `peinit.config.reload.deferred`: a reload during the boot window left
/// definitions pending on boot-plan members whose launch had not been
/// attempted. A boot executes against its snapshot (§3.7), so those members
/// start from the plan and take the change once the window has closed
/// (PEI-350).
pub fn encode_registry_reload_deferred_event(
    services: &[String],
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("graph.services", services);
    payload.finish(CONFIG_RELOAD_DEFERRED)
}

/// `peinit.config.reload.applied`: a reload ran, and from now on the
/// registry as it then stood is the running configuration — or it failed,
/// and nothing changed.
///
/// Written for an explicit `reload-config`, and for the one reload that
/// follows the boot window to apply what the reloads during it deferred
/// (PEI-350). `deferred` is that reload's deferred set, and only it carries
/// `graph.services`. The counts are written only when the reload succeeded:
/// a failed reload changed nothing, and zeros would say it had.
pub fn encode_config_reload_applied_event(
    deferred: Option<&[String]>,
    outcome: Result<&ReloadConfigOutcome, &ReloadConfigError>,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("outcome.success", outcome.is_ok());
    match outcome {
        Ok(outcome) => {
            let summary = &outcome.summary;
            payload.set_opt("graph.services", deferred);
            payload.set("graph.counts.added", summary.added.len() as u64);
            payload.set("graph.counts.updated", summary.updated.len() as u64);
            payload.set("graph.counts.restored", summary.restored.len() as u64);
            payload.set(
                "graph.counts.marked-removed",
                summary.marked_removed.len() as u64,
            );
            payload.set("graph.counts.discarded", summary.discarded.len() as u64);
            payload.set(
                "graph.counts.undecodable",
                summary.undecodable.len() as u64,
            );
        }
        Err(error) => {
            payload.set_opt("outcome.detail", reload_error_detail(error));
            payload.set_opt("graph.services", deferred);
        }
    }
    payload.finish(CONFIG_RELOAD_APPLIED)
}

/// Why a reload failed, in words for a person. The findings of a failed
/// validation are each their own `peinit.graph.validation.failed`.
fn reload_error_detail(error: &ReloadConfigError) -> Option<String> {
    match error {
        ReloadConfigError::Validation(failure) => Some(format!(
            "the service graph failed validation with {} finding(s)",
            failure.findings.len()
        )),
        ReloadConfigError::Registry(error) => error.text().map(str::to_string),
        ReloadConfigError::ServiceTable(_) => {
            Some("the new service table could not be applied".to_string())
        }
    }
}
