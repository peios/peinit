use std::collections::{BTreeMap, BTreeSet};

use crate::boot::phase2::Phase2BootPlan;
use crate::control::lifecycle::OnDemandStartDispatch;
use crate::ids::OperationId;
use crate::service::{ServiceDefinition, ServiceTable};

use super::build::{boot_context, on_demand_context};
use super::model::{GraphContextBuildError, GraphContextId, GraphExecutionContext};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GraphExecutionStore {
    pub(super) next_context_id: u64,
    pub(super) contexts: BTreeMap<GraphContextId, GraphExecutionContext>,
    pub(super) associations: BTreeMap<OperationId, BTreeSet<GraphContextId>>,
}

impl GraphExecutionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_boot_context(
        &mut self,
        plan: &Phase2BootPlan,
        definitions: &[ServiceDefinition],
    ) -> Result<GraphContextId, GraphContextBuildError> {
        let id = self.pending_context_id();
        let context = boot_context(id, plan, definitions)?;
        self.insert_context(context);
        Ok(id)
    }

    pub fn create_on_demand_context(
        &mut self,
        dispatch: &OnDemandStartDispatch,
        services: &ServiceTable,
    ) -> Result<GraphContextId, GraphContextBuildError> {
        let id = self.pending_context_id();
        let context = on_demand_context(id, dispatch, services)?;
        self.insert_context(context);
        Ok(id)
    }

    pub fn context(&self, context_id: GraphContextId) -> Option<&GraphExecutionContext> {
        self.contexts.get(&context_id)
    }

    /// How many contexts the store is holding.
    ///
    /// Exists so the retention behaviour can be asserted: contexts are
    /// internal, but "how many are there" is the whole question in PEI-364.
    pub fn context_count(&self) -> usize {
        self.contexts.len()
    }

    /// How many operations still point at a context.
    pub fn association_count(&self) -> usize {
        self.associations.len()
    }

    /// Contexts that are not drained and carry a level edge on `service` —
    /// the ones a `LEVEL=` arrival (or the publisher stopping) can unblock.
    ///
    /// A coarse filter, deliberately: `release_ready` re-derives the exact
    /// answer, so releasing a context whose edge turns out unsettled is a
    /// no-op, and this only has to avoid scanning every context on every
    /// datagram from a service nothing waits on.
    pub fn contexts_with_level_dependency_on(&self, service: &str) -> Vec<GraphContextId> {
        self.contexts
            .values()
            .filter(|context| !context.is_drained())
            .filter(|context| {
                context
                    .dependencies
                    .iter()
                    .any(|dependency| dependency.target == service && dependency.level.is_some())
            })
            .map(|context| context.id)
            .collect()
    }

    /// Whether `service`'s launch under `context_id` has been attempted or
    /// decided against. A retired or unknown context has nothing left to
    /// launch, so everything counts as attempted (PEI-350).
    pub fn boot_launch_attempted(&self, context_id: GraphContextId, service: &str) -> bool {
        self.contexts
            .get(&context_id)
            .is_none_or(|context| context.launch_attempted(service))
    }

    /// The members of `context_id` whose launch has not been attempted, in
    /// plan order; empty for a retired or unknown context.
    pub fn unattempted_launches(&self, context_id: GraphContextId) -> Vec<String> {
        self.contexts
            .get(&context_id)
            .map(GraphExecutionContext::unattempted_launches)
            .unwrap_or_default()
    }

    pub fn associated_contexts(&self, operation_id: OperationId) -> Vec<GraphContextId> {
        self.associations
            .get(&operation_id)
            .map(|contexts| contexts.iter().copied().collect())
            .unwrap_or_default()
    }

    fn pending_context_id(&self) -> GraphContextId {
        GraphContextId(self.next_context_id)
    }

    /// Drop every context whose members have all reached a terminal status,
    /// and the operation associations that pointed at them.
    ///
    /// A drained context can never dispatch another graph event, so it is
    /// bookkeeping with no reader. Nothing used to remove one, and the cost
    /// was two-sided: every boot and every explicit start leaked a context and
    /// a set of associations for the life of the process, and
    /// `apply_operation_terminal` walks *every* associated context — so on a
    /// machine up for months, where an operator or a script starts services
    /// regularly, the terminal path got steadily slower in PID 1's single
    /// thread (PEI-364).
    ///
    /// Retirement is a turn boundary rather than something
    /// `apply_operation_terminal` does inline. A context becomes drained the
    /// moment its last member goes terminal, but the callers of that method
    /// then walk the graph events it returned and call `release_ready` on the
    /// contexts they name — so dropping it there pulls the context out from
    /// under the rest of the same turn's work.
    ///
    /// Returns the retired ids, so a caller can say what it dropped.
    pub fn retire_drained_contexts(&mut self) -> Vec<GraphContextId> {
        let retired = self
            .contexts
            .values()
            .filter(|context| context.is_drained())
            .map(|context| context.id)
            .collect::<Vec<_>>();
        if retired.is_empty() {
            return retired;
        }
        for context_id in &retired {
            self.contexts.remove(context_id);
        }
        // An operation can be associated with more than one context — an
        // explicit start merging into an already-starting one is associated
        // with both — so an association is only gone once every context it
        // names has been retired.
        self.associations.retain(|_, contexts| {
            contexts.retain(|context_id| !retired.contains(context_id));
            !contexts.is_empty()
        });
        retired
    }

    fn insert_context(&mut self, context: GraphExecutionContext) {
        self.next_context_id = context.id.as_u64() + 1;
        for member in context.members.values() {
            self.associations
                .entry(member.operation_id)
                .or_default()
                .insert(context.id);
        }
        self.contexts.insert(context.id, context);
    }
}
