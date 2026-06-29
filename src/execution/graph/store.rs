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

    pub fn associated_contexts(&self, operation_id: OperationId) -> Vec<GraphContextId> {
        self.associations
            .get(&operation_id)
            .map(|contexts| contexts.iter().copied().collect())
            .unwrap_or_default()
    }

    fn pending_context_id(&self) -> GraphContextId {
        GraphContextId(self.next_context_id)
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
