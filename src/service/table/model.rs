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

pub(super) fn retains_definition_after_removal(state: ServiceState) -> bool {
    matches!(
        state,
        ServiceState::Starting
            | ServiceState::Active
            | ServiceState::Reloading
            | ServiceState::Backoff
            | ServiceState::Stopping
    )
}
