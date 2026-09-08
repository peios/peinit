//! Retiring an `OnFailure` chain once its handler has arrived.
//!
//! The chain that bounds an `OnFailure` cascade (§5.2) is recorded against the
//! handler that was started, and picked back up when *that* handler fails. So
//! a handler holds its membership entry and one of the sixteen depth slots for
//! as long as the entry lives — and the only things that used to clear it were
//! terminal or inactive states. A handler that started and stayed running held
//! its slot indefinitely, which meant the guard was consumed by the case that
//! worked: a degradation path handing off through several healthy layers
//! exhausted its own depth budget (PEI-362).
//!
//! Clearing on `Active` would fix that and break the guard. `Readiness=Alive`
//! reports Active the instant the process spawns, so two services naming each
//! other as handlers would hand off forever with no delay and no cost —
//! `consecutive_restart_failures` only moves on a restart, and an `OnFailure`
//! start is not one. That is exactly the crash-loop §5.2 says MUST be bounded.
//!
//! The distinction that matters is *arrived* versus *flapped*, and peinit
//! already has a name for it: dependent-satisfying for `RestartWindow`, which
//! is what resets a service's restart budget. A handler that sustains that has
//! demonstrably done its job, and the failure it was started for is over.
//!
//! This cannot live with the transition-driven clearing, because arrival is a
//! timer fact rather than a transition. It also cannot reuse
//! `due_restart_window_resets`, which filters on
//! `consecutive_restart_failures > 0` — an `OnFailure` handler's is usually
//! zero, so its deadline would never exist.

use crate::service::runtime::ServiceState;

use super::super::state::{Supervisor, SupervisorError};
use super::super::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// A chain entry that may be retired once its handler has held a window of
/// health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorOnFailureChainSettledDispatch {
    pub service: String,
    pub due_at_ns: u64,
}

impl Supervisor {
    /// When the earliest chain entry could be retired.
    pub(in crate::supervisor::operation_maintenance) fn next_on_failure_chain_settle_deadline_ns(
        &self,
    ) -> Option<u64> {
        self.on_failure_chain_settle_deadlines()
            .map(|settle| settle.due_at_ns)
            .min()
    }

    pub(in crate::supervisor::operation_maintenance) fn due_on_failure_chain_settles(
        &self,
        now_ns: u64,
    ) -> Vec<SupervisorOnFailureChainSettledDispatch> {
        self.on_failure_chain_settle_deadlines()
            .filter(|settle| settle.due_at_ns <= now_ns)
            .collect()
    }

    fn on_failure_chain_settle_deadlines(
        &self,
    ) -> impl Iterator<Item = SupervisorOnFailureChainSettledDispatch> + '_ {
        self.relationships
            .on_failure_chain_services()
            .into_iter()
            .filter_map(|service| {
                let runtime = self.services.runtime(&service)?;
                if runtime.state != ServiceState::Active {
                    return None;
                }
                let since_ns = runtime.dependent_satisfied_since_ns?;
                let definition = self.services.definition(&service)?;
                Some(SupervisorOnFailureChainSettledDispatch {
                    due_at_ns: since_ns.saturating_add(
                        definition.restart_window_secs.saturating_mul(NANOS_PER_SEC),
                    ),
                    service,
                })
            })
    }
}

pub(in crate::supervisor::operation_maintenance) fn settle_due_on_failure_chains(
    work: &mut SupervisorWork,
    due: Vec<SupervisorOnFailureChainSettledDispatch>,
    now_ns: u64,
) -> Result<Vec<SupervisorOnFailureChainSettledDispatch>, SupervisorError> {
    let mut settled = Vec::new();
    for candidate in due {
        // Re-validate against the work snapshot: an earlier step in this same
        // maintenance turn may have moved the service, and a handler that
        // stopped being Active has not arrived after all.
        if !still_settled(work, &candidate, now_ns) {
            continue;
        }
        work.relationships
            .clear_on_failure_chain(&candidate.service);
        settled.push(candidate);
    }
    Ok(settled)
}

fn still_settled(
    work: &SupervisorWork,
    candidate: &SupervisorOnFailureChainSettledDispatch,
    now_ns: u64,
) -> bool {
    let Some(runtime) = work.services.runtime(&candidate.service) else {
        return false;
    };
    if runtime.state != ServiceState::Active {
        return false;
    }
    let Some(since_ns) = runtime.dependent_satisfied_since_ns else {
        return false;
    };
    let Some(definition) = work.services.definition(&candidate.service) else {
        return false;
    };
    since_ns.saturating_add(definition.restart_window_secs.saturating_mul(NANOS_PER_SEC)) <= now_ns
}
