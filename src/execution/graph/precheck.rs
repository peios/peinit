use crate::ids::OperationId;

use super::model::{GraphContextId, GraphExecutionContext, GraphExecutionError, GraphMemberStatus};
use super::store::GraphExecutionStore;

impl GraphExecutionStore {
    pub fn apply_pre_start_check_passed(
        &mut self,
        operation_id: OperationId,
    ) -> Result<Vec<GraphContextId>, GraphExecutionError> {
        let Some(context_ids) = self.associations.get(&operation_id).cloned() else {
            return Ok(Vec::new());
        };
        let mut changed = Vec::new();
        for context_id in context_ids {
            let context = self
                .contexts
                .get_mut(&context_id)
                .ok_or(GraphExecutionError::UnknownContext { context_id })?;
            if mark_pre_start_check_passed(context, operation_id)? {
                changed.push(context_id);
            }
        }
        Ok(changed)
    }
}

fn mark_pre_start_check_passed(
    context: &mut GraphExecutionContext,
    operation_id: OperationId,
) -> Result<bool, GraphExecutionError> {
    let service = context
        .member_for_operation(operation_id)
        .ok_or(GraphExecutionError::UnknownAssociatedOperation { operation_id })?
        .service
        .clone();
    let member =
        context
            .members
            .get_mut(&service)
            .ok_or_else(|| GraphExecutionError::MissingMember {
                context_id: context.id,
                service: service.clone(),
            })?;
    if member.status.is_terminal() {
        return Err(GraphExecutionError::MemberAlreadyTerminal {
            context_id: context.id,
            service,
            status: member.status,
        });
    }
    if member.status == GraphMemberStatus::WaitingForDependencies {
        return Ok(false);
    }
    member.status = GraphMemberStatus::WaitingForDependencies;
    activate_dependencies(context, &service);
    Ok(true)
}

fn activate_dependencies(context: &mut GraphExecutionContext, service: &str) {
    let targets = context
        .dependencies
        .iter()
        .filter(|dependency| dependency.dependent == service)
        .map(|dependency| dependency.target.clone())
        .collect::<Vec<_>>();
    for target in targets {
        let Some(member) = context.members.get_mut(&target) else {
            continue;
        };
        if member.status == GraphMemberStatus::Dormant {
            member.status = GraphMemberStatus::WaitingForPreStartCheck;
        }
    }
}
