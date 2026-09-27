//! Deciding whether a console message may be written.
//!
//! Two gates, applied in order. See [`QuietLevel`] for why they stack rather
//! than form a scale.

use crate::init::QuietLevel;
use crate::service::ServiceTable;
use crate::service::runtime::ProcessPresence;

use super::ConsoleSeverity;

/// The terminal peinit itself writes to.
const CONSOLE_PATH: &str = "/dev/console";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuietPolicy {
    level: QuietLevel,
    /// A live service holds the console as its controlling terminal.
    console_is_owned: bool,
}

impl QuietPolicy {
    pub fn new(level: QuietLevel, console_is_owned: bool) -> Self {
        Self {
            level,
            console_is_owned,
        }
    }

    /// Evaluate against the current service table.
    pub fn evaluate(level: QuietLevel, services: &ServiceTable) -> Self {
        let console_is_owned =
            level.respects_terminal_ownership() && console_owned_by_a_live_service(services);
        Self::new(level, console_is_owned)
    }

    pub fn allows(self, severity: ConsoleSeverity) -> bool {
        // Ownership first, and it is the stricter of the two: an error is news
        // worth overriding a preference for, but it is not worth writing into
        // the middle of someone's login prompt. Only losing the machine is.
        if self.console_is_owned && severity < ConsoleSeverity::Critical {
            return false;
        }
        if self.level.suppresses_status() && severity < ConsoleSeverity::Error {
            return false;
        }
        true
    }
}

/// Whether any service that currently has a process holds peinit's console.
///
/// "Has a process" rather than "is Active": a service in `Starting` has already
/// been exec'd with the terminal on its standard streams, so the prompt can be
/// on screen before readiness is reported. Waiting for Active would leave
/// exactly the window this exists to close.
fn console_owned_by_a_live_service(services: &ServiceTable) -> bool {
    services.service_names().into_iter().any(|service| {
        let Some(runtime) = services.runtime(service) else {
            return false;
        };
        if runtime.state.process_presence() == ProcessPresence::None {
            return false;
        }
        services
            .definition(service)
            .and_then(|definition| definition.console_path.as_deref())
            .is_some_and(is_peinit_console)
    })
}

/// Whether a service's `TTYPath` names the same terminal peinit writes to.
///
/// Compared by device, not by string. `/dev/console` and `/dev/ttyS0` are two
/// names for one terminal on a serial-console machine, and a string comparison
/// would silently do nothing on exactly the images this matters most for.
fn is_peinit_console(tty_path: &str) -> bool {
    if tty_path == CONSOLE_PATH {
        return true;
    }
    match (device_of(tty_path), device_of(CONSOLE_PATH)) {
        (Some(a), Some(b)) => a == b,
        // Unstattable: fall back to refusing the match rather than assuming it.
        // Guessing wrong in this direction costs a scrambled line; guessing
        // wrong the other way costs the operator their console output.
        _ => false,
    }
}

#[cfg(feature = "peios-boundary")]
fn device_of(path: &str) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|metadata| metadata.rdev())
}

#[cfg(not(feature = "peios-boundary"))]
fn device_of(_path: &str) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests;
