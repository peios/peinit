//! `tty:released` — hand a terminal to whoever was queued for it.
//!
//! The trigger exists because the alternatives all say the wrong thing.
//! `Conflicts` fails both services, which on a console means losing the only
//! way into the machine. `Requires` would make a login prompt depend on
//! first-boot setup having run, which is false — it depends on the console
//! being free, and a setup flow that *failed* frees it just as well as one
//! that succeeded. `boot:settled` gets the timing right for the first service
//! to want the console and says nothing at all about the second.
//!
//! So the waiter names its terminal, asks to be woken when it comes free, and
//! this is what wakes it.

use std::collections::BTreeMap;

use crate::execution::start::StartExecutionDispatch;
use crate::service::ServiceTableTransition;
use crate::service::tty::{tty_holder, tty_release_candidate};
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::super::start::dispatch_tty_release_start;

/// Start the best waiter for every terminal let go of during this batch.
///
/// Driven off transitions rather than polled, so a terminal that nobody
/// touched costs nothing — and driven off *every* transition, so it does not
/// matter whether the holder exited cleanly, crashed, was stopped by an
/// administrator or was evicted by a conflict. All four free the device.
pub(super) fn apply_tty_release_starts(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    let mut dispatches = Vec::new();
    let mut released: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for transition in transitions {
        // From the transition, not from the table. A service that removed
        // its own definition and then exited has already lost its entry by
        // the time this runs — and that is not a corner case, it is what a
        // first-boot setup flow does on the way out. Looking the terminal
        // up here found nothing, so the console was never handed on and
        // the machine sat on a screen whose owner had gone.
        let Some(tty) = transition.released_tty.clone() else {
            continue;
        };
        released
            .entry(tty)
            .or_default()
            .push(transition.event.service.clone());
    }

    for (tty, released_by) in released {
        // Somebody else may have taken it in the meantime — two services can
        // leave a holding state in one batch, and the second one's release is
        // not an offer of a terminal the first's waiter already has. The empty
        // exclusion is "nobody": a service name is never empty, so this asks
        // whether *anyone at all* holds the device.
        if tty_holder(&work.services, &tty, "").is_some() {
            continue;
        }
        let Some(candidate) = tty_release_candidate(&work.services, &tty, &released_by) else {
            continue;
        };
        let candidate = candidate.to_string();
        dispatches.extend(dispatch_tty_release_start(
            work,
            &candidate,
            observed_at_ns,
            max_parallel_starts,
        )?);
    }
    Ok(dispatches)
}
