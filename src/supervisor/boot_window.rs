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
//! The spec asks only that the in-progress boot be unaffected, and refusing
//! every reload for the length of the window turned out to be the wrong
//! trade: the window closes in seconds on a real image, and install scripts
//! and operators alike reload or write the registry in exactly those
//! seconds. So a reload during the window RUNS, and applies as any reload
//! does, except to a boot-plan member whose launch has not been attempted
//! yet: that entry keeps the plan's definition and takes the new one as
//! pending — the same mechanism that pins a running service — so the boot
//! start is made from the snapshot. Those names are reported as `deferred`,
//! and once every planned launch has been attempted one more reload applies
//! them (a plain re-read: by then nothing is frozen), announced as the
//! coalesced reload so an observer can tell when the snapshot was let go of.

use crate::execution::graph::GraphContextId;

use super::Supervisor;

/// The definitions a reload during the boot window left pending on
/// not-yet-launched boot-plan members, for the one reload that follows the
/// window and applies them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeferredRegistryReload {
    /// Every service deferred by any reload during the window, sorted,
    /// each once.
    pub services: Vec<String>,
}

impl Supervisor {
    /// The boot plan is still executing: the Phase 2 graph context has a
    /// member whose launch has not been attempted yet.
    ///
    /// A retired context — dropped once drained, at a maintenance sweep —
    /// counts as drained, as does never having booted. So does a plan whose
    /// only live members are a service in Backoff and the dependents held
    /// for its restart (PEI-821): the boot has done its part, and the retry
    /// cycle must not hold the window open.
    pub fn boot_plan_in_progress(&self) -> bool {
        !self.frozen_boot_plan_members().is_empty()
    }

    /// The boot-plan members a reload must not replace: those whose launch
    /// has not been attempted (§3.7). Empty once the window has closed.
    ///
    /// The graph knows which members are decided (terminal, awaiting a
    /// restart, or held only on members that are); it does not know which
    /// dispatched members have actually launched, because a dispatched
    /// job can still be queued and the activation snapshot is taken at the
    /// launch. That half is the job store's: a member whose reserved job
    /// exists and has left `Created` has been attempted, whatever happens
    /// to it next. A member with no job record yet has not: a held
    /// member's job is only created when it is released.
    pub fn frozen_boot_plan_members(&self) -> Vec<String> {
        let Some(context_id) = self.boot_plan_context else {
            return Vec::new();
        };
        let Some(context) = self.graph.context(context_id) else {
            return Vec::new();
        };
        self.graph
            .unattempted_launches(context_id)
            .into_iter()
            .filter(|service| {
                let launched = context
                    .members
                    .get(service)
                    .and_then(|member| member.reserved_job_id)
                    .and_then(|job_id| self.jobs.get(job_id))
                    .is_some_and(|job| job.state != crate::job::JobState::Created);
                !launched
            })
            .collect()
    }

    pub(super) fn note_boot_plan_context(&mut self, context_id: GraphContextId) {
        self.boot_plan_context = Some(context_id);
    }

    /// Record the definitions a reload left pending on frozen members.
    pub(super) fn record_deferred_definitions(&mut self, services: &[String]) {
        if services.is_empty() {
            return;
        }
        let deferred = self.deferred_registry_reload.get_or_insert_default();
        deferred.services.extend(services.iter().cloned());
        deferred.services.sort();
        deferred.services.dedup();
    }

    /// Whether a reload during the window deferred something that the
    /// reload after it still has to apply.
    pub fn has_deferred_registry_reload(&self) -> bool {
        self.deferred_registry_reload.is_some()
    }

    /// What was deferred, once the boot window has closed; `None` while it
    /// is open or when nothing was deferred. Taking it clears it, so the
    /// caller owns the one reload it stands for.
    pub fn take_due_deferred_registry_reload(&mut self) -> Option<DeferredRegistryReload> {
        if self.boot_plan_in_progress() {
            return None;
        }
        self.deferred_registry_reload.take()
    }
}
