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

    /// The operation failed, but its service is restart-eligible and has
    /// gone to Backoff: hold the member rather than failing it, so its
    /// dependents wait for the restart instead of failing (§6.1).
    ///
    /// Nothing is released or failed here, deliberately. The member's
    /// operation is over, but the *fact* its dependents wait on — the
    /// service being up — is still undecided, exactly as a level not yet
    /// published is. Returns the contexts that now hold a member.
    pub fn apply_operation_awaiting_restart(
        &mut self,
        operation_id: OperationId,
    ) -> Result<Vec<GraphContextId>, GraphExecutionError> {
        let Some(context_ids) = self.associations.get(&operation_id).cloned() else {
            return Ok(Vec::new());
        };
        let mut held = Vec::new();
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
            mark_awaiting_restart(context, &service)?;
            // The hold begins here, with no release pass to notice it:
            // record what the dependents are now held on.
            context.note_holds_without_clock();
            held.push(context_id);
        }
        Ok(held)
    }

    /// Whether any live context holds `service` for a restart.
    pub fn is_awaiting_restart(&self, service: &str) -> bool {
        self.contexts.values().any(|context| {
            context
                .members
                .get(service)
                .is_some_and(|member| member.status == GraphMemberStatus::AwaitingRestart)
        })
    }

    /// Every service some live context holds for a restart, each once.
    pub fn awaiting_restart_services(&self) -> Vec<String> {
        let mut services = self
            .contexts
            .values()
            .flat_map(|context| context.members.values())
            .filter(|member| member.status == GraphMemberStatus::AwaitingRestart)
            .map(|member| member.service.clone())
            .collect::<Vec<_>>();
        services.sort();
        services.dedup();
        services
    }

    /// The restart a held member waited for has been decided: the service
    /// reached a dependent-satisfying state (`Satisfied`), or it will not
    /// be coming back (`Failed`).
    ///
    /// The member keeps the operation it was created with, which is long
    /// terminal, so the event for the member itself is only a handle on
    /// the context; a `Failed` settlement additionally propagates to the
    /// hard dependents, whose operations *are* still pending and are what
    /// the caller has to fail.
    pub fn settle_awaiting_restart(
        &mut self,
        service: &str,
        outcome: GraphTerminalOutcome,
    ) -> Result<Vec<GraphExecutionEvent>, GraphExecutionError> {
        let mut events = Vec::new();
        for context in self.contexts.values_mut() {
            let Some(member) = context.members.get(service) else {
                continue;
            };
            if member.status != GraphMemberStatus::AwaitingRestart {
                continue;
            }
            let operation_id = member.operation_id;
            mark_terminal(context, service, outcome)?;
            events.push(GraphExecutionEvent {
                context_id: context.id,
                service: service.to_string(),
                operation_id,
                outcome,
            });
            if outcome == GraphTerminalOutcome::Failed {
                events.extend(propagate_hard_dependency_failure(context, service)?);
            }
        }
        Ok(events)
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
        // This operation may be the restart an earlier context's member was
        // held for: the relaunch out of Backoff runs under its own context
        // and operation, so the hold is settled by *service*, not by
        // association. The events come back through the same list, so
        // whatever releases or fails after this terminal reaches the held
        // dependents too (PEI-821).
        events.extend(self.settle_awaiting_restart(&service, outcome)?);
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

fn mark_awaiting_restart(
    context: &mut GraphExecutionContext,
    service: &str,
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
    member.status = GraphMemberStatus::AwaitingRestart;
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
