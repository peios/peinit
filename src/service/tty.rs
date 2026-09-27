//! Who owns a terminal, and who gets it next.
//!
//! A `TTYPath` names a device that has exactly one owner at a time. Two
//! services writing to the same tty do not fail — they interleave, which on a
//! console produces a login prompt with a progress line through the middle of
//! it and a password echoed into somebody else's output. So peinit hands the
//! device to one service and makes the others wait.
//!
//! Waiting is not `Conflicts`. `Conflicts` fails both services when they meet
//! in one boot, and the services that want a terminal are the interactive ones
//! — a login prompt, an installer, a first-boot setup flow. Failing both of
//! those means a machine with no way in.
//!
//! Nor is it a dependency. A login prompt does not need first-boot setup to
//! have run in order to work; it needs the console to be free. Expressing that
//! as `Requires` would mean naming every service that might ever hold the
//! console, and rewriting each waiter every time that set changed — and it
//! would say the wrong thing besides, because a holder that fails still
//! releases the terminal and the waiter still wants it.
//!
//! What is left is a queue, and this module is the whole of its law:
//! [`holds_tty`] says who is in possession, [`tty_holder`] finds
//! them, and [`tty_release_candidate`] picks who takes over.

use super::ServiceDefinition;
use super::runtime::ServiceState;
use super::table::ServiceTable;

/// Whether a service in this state may still have a process on its terminal.
///
/// `Backoff` counts as holding, and that is the point of stating this as its
/// own function rather than reusing "is terminal". A service between restart
/// attempts is coming back — handing its console to somebody else during the
/// gap produces two owners a second later, which is the exact thing this
/// exists to prevent.
///
/// `Abandoned` does not count, and that one is a genuine compromise. The
/// process survived SIGKILL and may well still be writing. But peinit has no
/// remaining way to reclaim the device from it, so treating it as held would
/// mean the terminal is lost for the rest of the boot — and a console nobody
/// can log in on is a worse outcome than a console with a stuck process on it.
pub fn holds_tty(state: ServiceState) -> bool {
    match state {
        ServiceState::Starting
        | ServiceState::Active
        | ServiceState::Reloading
        | ServiceState::Stopping
        | ServiceState::Backoff => true,
        ServiceState::Inactive
        | ServiceState::Completed
        | ServiceState::Failed
        | ServiceState::Skipped
        | ServiceState::Abandoned => false,
    }
}

/// The service currently in possession of `tty`, ignoring `except`.
///
/// `except` is the service asking. Without it a service being restarted would
/// find its own previous incarnation in the way.
pub fn tty_holder<'a>(services: &'a ServiceTable, tty: &str, except: &str) -> Option<&'a str> {
    services
        .service_names()
        .into_iter()
        .filter(|service| *service != except)
        .find(|service| {
            let Some(entry) = services.get(service) else {
                return false;
            };
            entry.definition.console_path.as_deref() == Some(tty) && holds_tty(entry.runtime.state)
        })
}

/// Who should be given `tty` now that it is free, if anybody.
///
/// Only services that asked to be woken this way — `tty:released` — are
/// considered. Highest `TTYPrecedence` wins; equal precedence breaks on
/// service name, so an operator who states no preference still gets the same
/// machine on every boot rather than whichever definition the registry
/// happened to enumerate first.
///
/// `released_by` names the services whose exit freed the terminal, and they
/// are never candidates for it. A service that just stopped is exactly the one
/// with the strongest claim on its own console, so without this the
/// highest-precedence holder restarts itself on every exit — a restart policy
/// that ignores the restart budget, and one no waiter ever gets past.
/// Relaunching a service that stopped is `RestartPolicy`'s job.
///
/// One winner, not all of them. Starting the whole queue would have every
/// loser immediately skipped again by the ordinary admission rule — the same
/// outcome, reached noisily, with a failed-looking service for each.
pub fn tty_release_candidate<'a>(
    services: &'a ServiceTable,
    tty: &str,
    released_by: &[String],
) -> Option<&'a str> {
    let mut best: Option<(&str, u32)> = None;
    for service in services.service_names() {
        let Some(entry) = services.get(service) else {
            continue;
        };
        if !waits_for_tty(&entry.definition, tty) {
            continue;
        }
        if released_by.iter().any(|name| name == service) {
            continue;
        }
        // A service already running, or on its way there, is not waiting.
        if holds_tty(entry.runtime.state) {
            continue;
        }
        // A definition the registry no longer carries cannot be started, so
        // offering it the terminal would only park the device on a service
        // that is about to be dropped.
        if entry.definition_removed {
            continue;
        }
        let precedence = entry.definition.console_precedence;
        if best.is_none_or(|(name, incumbent)| {
            precedence > incumbent || (precedence == incumbent && service < name)
        }) {
            best = Some((service, precedence));
        }
    }
    best.map(|(service, _)| service)
}

/// Whether `definition` is one of the services queued on `tty`.
fn waits_for_tty(definition: &ServiceDefinition, tty: &str) -> bool {
    !definition.disabled
        && definition.console_path.as_deref() == Some(tty)
        && definition.has_tty_released_trigger()
}

#[cfg(test)]
mod tests;
