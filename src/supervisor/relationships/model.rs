use std::collections::{BTreeMap, BTreeSet};

use crate::execution::graph::GraphContextId;
use crate::supervisor::dispatch::{
    SupervisorOnFailureLoopSuppressedDispatch, SupervisorOnFailureLoopSuppressionReason,
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::supervisor) struct RelationshipStore {
    conflict_contexts: BTreeMap<GraphContextId, PendingConflictContext>,
    on_failure_chains: BTreeMap<String, OnFailureChain>,
    audit_events: Vec<SupervisorOnFailureLoopSuppressedDispatch>,
}

impl RelationshipStore {
    pub(in crate::supervisor) fn new() -> Self {
        Self::default()
    }

    pub(super) fn record_conflict_context(
        &mut self,
        context_id: GraphContextId,
        start_services: BTreeSet<String>,
        blockers: BTreeSet<String>,
    ) {
        self.conflict_contexts.insert(
            context_id,
            PendingConflictContext {
                context_id,
                start_services,
                blockers,
            },
        );
    }

    pub(super) fn pending_conflict_contexts(&self) -> Vec<PendingConflictContext> {
        self.conflict_contexts.values().cloned().collect()
    }

    pub(super) fn remove_conflict_context(
        &mut self,
        context_id: GraphContextId,
    ) -> Option<PendingConflictContext> {
        self.conflict_contexts.remove(&context_id)
    }

    pub(super) fn record_on_failure_chain(&mut self, service: String, chain: OnFailureChain) {
        self.on_failure_chains.insert(service, chain);
    }

    pub(super) fn take_on_failure_chain(&mut self, service: &str) -> Option<OnFailureChain> {
        self.on_failure_chains.remove(service)
    }

    pub(in crate::supervisor) fn clear_on_failure_chain(&mut self, service: &str) {
        self.on_failure_chains.remove(service);
    }

    /// The services currently holding a chain entry.
    ///
    /// Needed so the maintenance pass can ask which of them have *arrived* —
    /// see [`crate::supervisor::operation_maintenance`]. A chain is an
    /// obligation on the service that was started as a handler, and nothing
    /// else enumerates them.
    pub(in crate::supervisor) fn on_failure_chain_services(&self) -> Vec<String> {
        self.on_failure_chains.keys().cloned().collect()
    }

    pub(super) fn record_audit_event(&mut self, event: SupervisorOnFailureLoopSuppressedDispatch) {
        self.audit_events.push(event);
    }

    pub(in crate::supervisor) fn drain_audit_events(
        &mut self,
    ) -> Vec<SupervisorOnFailureLoopSuppressedDispatch> {
        std::mem::take(&mut self.audit_events)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingConflictContext {
    pub context_id: GraphContextId,
    pub start_services: BTreeSet<String>,
    pub blockers: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OnFailureChain {
    services: Vec<String>,
}

impl OnFailureChain {
    pub(super) const MAX_DEPTH: usize = 16;

    pub(super) fn root() -> Self {
        Self {
            services: Vec::new(),
        }
    }

    pub(super) fn with_handler(
        &self,
        service: &str,
    ) -> Result<Self, SupervisorOnFailureLoopSuppressionReason> {
        if self.services.len() >= Self::MAX_DEPTH {
            return Err(SupervisorOnFailureLoopSuppressionReason::MaxDepth {
                max_depth: Self::MAX_DEPTH,
            });
        }
        if self.services.iter().any(|existing| existing == service) {
            return Err(SupervisorOnFailureLoopSuppressionReason::Cycle);
        }
        let mut services = self.services.clone();
        services.push(service.to_string());
        Ok(Self { services })
    }

    pub(super) fn path_with_attempted_handler(&self, service: &str) -> Vec<String> {
        let mut path = self.services.clone();
        path.push(service.to_string());
        path
    }
}
