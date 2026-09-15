//! The boot window: a boot executes against its snapshot.
//!
//! §3.7: "The entire boot executes against this snapshot. Registry changes
//! made during boot (by install scripts, post-hooks, etc.) MUST NOT affect
//! the in-progress boot." The plan and the graph were always snapshotted, but
//! the registry watches are armed as the runtime loop is built — while
//! boot-plan services are still Starting or Inactive — and every watch event
//! ran a full reload with no in-boot guard. A boot-plan service that had not
//! started yet then picked up a new `ImagePath`, `Identity` or `Environment`
//! mid-boot, so a boot could start half its services from one configuration
//! and half from another (PEI-350). Not hypothetical: a package transaction
//! running as a boot-triggered Oneshot writes service definitions, and so
//! does any first-boot provisioning service.
//!
//! The reload is therefore gated on the boot plan having drained — every
//! member of the Phase 2 graph context terminal — and whatever arrived
//! during the boot is coalesced into ONE reload afterwards, which reaches the
//! same end state a reload at the time would have, without mutating a boot
//! in flight. The plan always drains: a member that never starts is failed
//! by the pending-operation timeout.

use crate::execution::graph::GraphContextId;

use super::Supervisor;

/// What arrived while the boot plan was still draining, for the one reload
/// that follows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeferredRegistryReload {
    /// The registry watch descriptor the last deferred batch came in on, if
    /// any came in on one.
    pub watch_fd: Option<i32>,
    /// Registry watch events deferred, across every batch.
    pub watch_events: usize,
    /// Whether any deferred batch reported an overflow. Immaterial to the
    /// reload, which is always a full re-read, but part of the record.
    pub overflow: bool,
    /// Explicit `reload-config` requests refused during the window. Their
    /// intent is honoured by the coalesced reload.
    pub explicit_requests: usize,
}

impl Supervisor {
    /// The boot plan is still executing: the Phase 2 graph context has a
    /// member whose launch has not been attempted yet.
    ///
    /// A retired context — dropped once drained, at a maintenance sweep —
    /// counts as drained, as does never having booted. So does a plan whose
    /// only live members are a service in Backoff and the dependents held
    /// for its restart (PEI-821): the boot has done its part, and the retry
    /// cycle must not hold every reload open.
    pub fn boot_plan_in_progress(&self) -> bool {
        self.boot_plan_context.is_some_and(|context_id| {
            self.graph
                .context(context_id)
                .is_some_and(|context| !context.has_attempted_every_launch())
        })
    }

    pub(super) fn note_boot_plan_context(&mut self, context_id: GraphContextId) {
        self.boot_plan_context = Some(context_id);
    }

    /// Record a registry watch batch that arrived during the boot window.
    pub fn defer_registry_watch_reload(&mut self, watch_fd: i32, events: usize, overflow: bool) {
        let deferred = self.deferred_registry_reload.get_or_insert_default();
        deferred.watch_fd = Some(watch_fd);
        deferred.watch_events += events;
        deferred.overflow |= overflow;
    }

    fn defer_explicit_reload(&mut self) {
        self.deferred_registry_reload
            .get_or_insert_default()
            .explicit_requests += 1;
    }

    /// Whether a reload is waiting on the boot plan.
    pub fn has_deferred_registry_reload(&self) -> bool {
        self.deferred_registry_reload.is_some()
    }

    /// The deferred reload, once the boot plan has drained; `None` while it
    /// is still draining or when nothing was deferred. Taking it clears it,
    /// so the caller owns the one reload it stands for.
    pub fn take_due_deferred_registry_reload(&mut self) -> Option<DeferredRegistryReload> {
        if self.boot_plan_in_progress() {
            return None;
        }
        self.deferred_registry_reload.take()
    }

    /// Refuse a reload during the boot window, recording that one was asked
    /// for so the coalesced reload afterwards is not skipped.
    pub(super) fn refuse_reload_during_boot_window(&mut self) -> bool {
        if !self.boot_plan_in_progress() {
            return false;
        }
        self.defer_explicit_reload();
        true
    }
}
