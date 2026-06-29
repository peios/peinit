use crate::ids::OperationId;

use super::model::{
    GraphExecutionContext, GraphExecutionError, GraphMemberStatus, GraphPrunedOperation,
};
use super::store::GraphExecutionStore;

impl GraphExecutionStore {
    pub fn prune_dormant_dependencies(
        &mut self,
        operation_id: OperationId,
    ) -> Result<Vec<GraphPrunedOperation>, GraphExecutionError> {
        let Some(context_ids) = self.associations.get(&operation_id).cloned() else {
            return Ok(Vec::new());
        };
        let mut pruned = Vec::new();
        for context_id in context_ids {
            let context = self
                .contexts
                .get_mut(&context_id)
                .ok_or(GraphExecutionError::UnknownContext { context_id })?;
            let service = context
                .member_for_operation(operation_id)
                .ok_or(GraphExecutionError::UnknownAssociatedOperation { operation_id })?
                .service
                .clone();
            pruned.extend(prune_dependencies_for_service(context, &service)?);
        }
        Ok(pruned)
    }
}

fn prune_dependencies_for_service(
    context: &mut GraphExecutionContext,
    service: &str,
) -> Result<Vec<GraphPrunedOperation>, GraphExecutionError> {
    let targets = context
        .dependencies
        .iter()
        .filter(|dependency| dependency.dependent == service)
        .map(|dependency| dependency.target.clone())
        .collect::<Vec<_>>();
    let mut pruned = Vec::new();
    for target in targets {
        pruned.extend(prune_dormant_subtree(context, &target)?);
    }
    Ok(pruned)
}

fn prune_dormant_subtree(
    context: &mut GraphExecutionContext,
    service: &str,
) -> Result<Vec<GraphPrunedOperation>, GraphExecutionError> {
    if !can_prune_dormant_member(context, service) {
        return Ok(Vec::new());
    }
    let operation_id =
        {
            let member = context.members.get_mut(service).ok_or_else(|| {
                GraphExecutionError::MissingMember {
                    context_id: context.id,
                    service: service.to_string(),
                }
            })?;
            member.status = GraphMemberStatus::Pruned;
            member.operation_id
        };
    let mut pruned = vec![GraphPrunedOperation {
        context_id: context.id,
        service: service.to_string(),
        operation_id,
    }];
    pruned.extend(prune_dependencies_for_service(context, service)?);
    Ok(pruned)
}

fn can_prune_dormant_member(context: &GraphExecutionContext, service: &str) -> bool {
    let Some(member) = context.members.get(service) else {
        return false;
    };
    if member.status != GraphMemberStatus::Dormant {
        return false;
    }
    context
        .dependencies
        .iter()
        .filter(|dependency| dependency.target == service)
        .filter_map(|dependency| context.members.get(&dependency.dependent))
        .all(|dependent| {
            matches!(
                dependent.status,
                GraphMemberStatus::Dormant
                    | GraphMemberStatus::Satisfied
                    | GraphMemberStatus::Failed
                    | GraphMemberStatus::Pruned
            )
        })
}
