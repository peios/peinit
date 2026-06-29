use crate::ids::OperationId;
use crate::service::ServiceDependencyKind;

use super::model::{
    GraphContextId, GraphExecutionContext, GraphExecutionError, GraphExecutionEvent,
    GraphMemberStatus, GraphTerminalOutcome,
};
use super::store::GraphExecutionStore;

impl GraphExecutionStore {
    pub fn apply_operation_satisfied(
        &mut self,
        operation_id: OperationId,
    ) -> Result<Vec<GraphExecutionEvent>, GraphExecutionError> {
        self.apply_operation_terminal(operation_id, GraphTerminalOutcome::Satisfied)
    }

    pub fn apply_operation_failed(
        &mut self,
        operation_id: OperationId,
    ) -> Result<Vec<GraphExecutionEvent>, GraphExecutionError> {
        self.apply_operation_terminal(operation_id, GraphTerminalOutcome::Failed)
    }

    fn apply_operation_terminal(
        &mut self,
        operation_id: OperationId,
        outcome: GraphTerminalOutcome,
    ) -> Result<Vec<GraphExecutionEvent>, GraphExecutionError> {
        let Some(context_ids) = self.associations.get(&operation_id).cloned() else {
            return Ok(Vec::new());
        };
        let mut events = Vec::new();
        for context_id in context_ids {
            events.extend(self.apply_context_terminal(context_id, operation_id, outcome)?);
        }
        Ok(events)
    }

    fn apply_context_terminal(
        &mut self,
        context_id: GraphContextId,
        operation_id: OperationId,
        outcome: GraphTerminalOutcome,
    ) -> Result<Vec<GraphExecutionEvent>, GraphExecutionError> {
        let context = self
            .contexts
            .get_mut(&context_id)
            .ok_or(GraphExecutionError::UnknownContext { context_id })?;
        let service = context
            .member_for_operation(operation_id)
            .ok_or(GraphExecutionError::UnknownAssociatedOperation { operation_id })?
            .service
            .clone();
        mark_terminal(context, &service, outcome)?;

        let mut events = vec![GraphExecutionEvent {
            context_id,
            service: service.clone(),
            operation_id,
            outcome,
        }];
        if outcome == GraphTerminalOutcome::Failed {
            events.extend(propagate_hard_dependency_failure(context, &service)?);
        }
        Ok(events)
    }
}

fn mark_terminal(
    context: &mut GraphExecutionContext,
    service: &str,
    outcome: GraphTerminalOutcome,
) -> Result<(), GraphExecutionError> {
    let member =
        context
            .members
            .get_mut(service)
            .ok_or_else(|| GraphExecutionError::MissingMember {
                context_id: context.id,
                service: service.to_string(),
            })?;
    if member.status.is_terminal() {
        return Err(GraphExecutionError::MemberAlreadyTerminal {
            context_id: context.id,
            service: service.to_string(),
            status: member.status,
        });
    }
    member.status = match outcome {
        GraphTerminalOutcome::Satisfied => GraphMemberStatus::Satisfied,
        GraphTerminalOutcome::Failed => GraphMemberStatus::Failed,
    };
    Ok(())
}

fn propagate_hard_dependency_failure(
    context: &mut GraphExecutionContext,
    failed_service: &str,
) -> Result<Vec<GraphExecutionEvent>, GraphExecutionError> {
    let dependents = hard_dependents(context, failed_service);
    let mut events = Vec::new();
    for dependent in dependents {
        let operation_id = context
            .members
            .get(&dependent)
            .ok_or_else(|| GraphExecutionError::MissingMember {
                context_id: context.id,
                service: dependent.clone(),
            })?
            .operation_id;
        mark_terminal(context, &dependent, GraphTerminalOutcome::Failed)?;
        events.push(GraphExecutionEvent {
            context_id: context.id,
            service: dependent.clone(),
            operation_id,
            outcome: GraphTerminalOutcome::Failed,
        });
        events.extend(propagate_hard_dependency_failure(context, &dependent)?);
    }
    Ok(events)
}

fn hard_dependents(context: &GraphExecutionContext, failed_service: &str) -> Vec<String> {
    context
        .dependencies
        .iter()
        .filter(|dependency| dependency.target == failed_service)
        .filter(|dependency| {
            matches!(
                dependency.kind,
                ServiceDependencyKind::Requires | ServiceDependencyKind::BindsTo
            )
        })
        .filter_map(|dependency| {
            let member = context.members.get(&dependency.dependent)?;
            matches!(
                member.status,
                GraphMemberStatus::WaitingForDependencies | GraphMemberStatus::Running
            )
            .then(|| dependency.dependent.clone())
        })
        .collect()
}
