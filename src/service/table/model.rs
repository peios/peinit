use crate::service::definition::ServiceDefinition;
use crate::service::runtime::{
    ServiceRuntimeSnapshot, ServiceState, ServiceTransitionError, ServiceTransitionEvent,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEntry {
    pub definition: ServiceDefinition,
    pub pending_definition: Option<ServiceDefinition>,
    pub runtime: ServiceRuntimeSnapshot,
    pub definition_removed: bool,
}

impl ServiceEntry {
    /// Does this entry satisfy a dependent asking for `level`?
    ///
    /// `None` is the ordinary dependency: the service need only be in a
    /// state that satisfies dependents. `Some(level)` additionally
    /// requires that the service has published exactly that level with
    /// `LEVEL=`, which is what `Requires = ["netd:routed"]` means.
    ///
    /// Exact match rather than "at least this level": peinit has no
    /// ordering over another daemon's vocabulary and must not invent one.
    /// netd knows that `routed` implies `addressed`; peinit does not, and
    /// a dependent that wants either should say so with two entries once
    /// that is expressible. Recorded here because the alternative — a
    /// central table of level orderings — is exactly the coupling this
    /// design avoided by namespacing levels per publisher.
    pub fn satisfies(&self, level: Option<&str>) -> bool {
        if !self.runtime.state.satisfies_dependents() {
            return false;
        }
        match level {
            None => true,
            Some(wanted) => self.runtime.level.as_deref() == Some(wanted),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceActivationSnapshot {
    pub service: String,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub definition: ServiceDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTableTransition {
    pub event: ServiceTransitionEvent,
    pub discarded_definition_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartBackoffDeadline {
    pub service: String,
    pub due_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartWindowResetDeadline {
    pub service: String,
    pub due_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceReloadSummary {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub restored: Vec<String>,
    pub marked_removed: Vec<String>,
    pub discarded: Vec<String>,
}

impl ServiceReloadSummary {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.updated.is_empty()
            && self.restored.is_empty()
            && self.marked_removed.is_empty()
            && self.discarded.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceTableError {
    DuplicateService { service: String },
    UnknownService { service: String },
    DefinitionRemoved { service: String },
    Transition(ServiceTransitionError),
}

/// The entry is kept alive after its definition is withdrawn, because there is
/// an instance still to supervise.
///
/// That is the whole reason for retention, which is why `Backoff` is not here.
/// A service between restart attempts has no process — `process_presence` says
/// so — and nothing left to restart it from, so there is nothing to supervise
/// and nothing to wait for. Retaining it was a permanent leak: the entry stayed
/// alive because it was in Backoff, and could never leave Backoff because
/// `restart_backoff_deadlines` skips definition-removed entries. It sat in
/// `status` describing a restart that would never happen, refused every
/// lifecycle command with UNKNOWN_SERVICE, and held its stored descriptors open
/// in PID 1 for the life of the process (PEI-346).
///
/// `Starting` stays: its process is `Optional`, not absent, so there may well
/// be one to drain.
pub(super) fn retains_definition_after_removal(state: ServiceState) -> bool {
    matches!(
        state,
        ServiceState::Starting
            | ServiceState::Active
            | ServiceState::Reloading
            | ServiceState::Stopping
    )
}
