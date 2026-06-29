use crate::service::ServiceDependencyKind;

use super::model::{
    GraphContextId, GraphExecutionContext, GraphExecutionError, GraphMemberStatus,
    ReadyGraphOperation, ReadyGraphOperationAction,
};
use super::store::GraphExecutionStore;

impl GraphExecutionStore {
    pub fn release_ready(
        &mut self,
        context_id: GraphContextId,
        max_parallel_starts: u32,
    ) -> Result<Vec<ReadyGraphOperation>, GraphExecutionError> {
        if max_parallel_starts == 0 {
            return Err(GraphExecutionError::InvalidMaxParallelStarts);
        }
        let available_slots = self.available_slots(context_id, max_parallel_starts)?;
        let ready = self.ready_members(context_id, available_slots)?;
        let context = self
            .contexts
            .get_mut(&context_id)
            .ok_or(GraphExecutionError::UnknownContext { context_id })?;

        for (service, _) in &ready {
            context
                .members
                .get_mut(service)
                .ok_or_else(|| GraphExecutionError::MissingMember {
                    context_id,
                    service: service.clone(),
                })?
                .status = GraphMemberStatus::Running;
        }

        let mut operations = Vec::new();
        for (service, action) in ready {
            let member = context.members.get(&service).ok_or_else(|| {
                GraphExecutionError::MissingMember {
                    context_id,
                    service: service.clone(),
                }
            })?;
            operations.push(ReadyGraphOperation {
                context_id,
                service,
                operation_id: member.operation_id,
                reserved_job_id: member.reserved_job_id,
                transition_cause: member.transition_cause,
                action,
            });
        }

        Ok(operations)
    }

    fn available_slots(
        &self,
        context_id: GraphContextId,
        max_parallel_starts: u32,
    ) -> Result<usize, GraphExecutionError> {
        let context = self
            .contexts
            .get(&context_id)
            .ok_or(GraphExecutionError::UnknownContext { context_id })?;
        let running = context
            .members
            .values()
            .filter(|member| member.status == GraphMemberStatus::Running)
            .count();
        Ok((max_parallel_starts as usize).saturating_sub(running))
    }

    fn ready_members(
        &self,
        context_id: GraphContextId,
        limit: usize,
    ) -> Result<Vec<(String, ReadyGraphOperationAction)>, GraphExecutionError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let context = self
            .contexts
            .get(&context_id)
            .ok_or(GraphExecutionError::UnknownContext { context_id })?;
        let mut ready = Vec::new();
        for member in context.members.values() {
            let action = match member.status {
                GraphMemberStatus::WaitingForPreStartCheck => {
                    Some(ReadyGraphOperationAction::PreStartCheck)
                }
                GraphMemberStatus::WaitingForDependencies
                    if dependencies_settled(context, &member.service)? =>
                {
                    Some(ReadyGraphOperationAction::Start)
                }
                _ => None,
            };
            if let Some(action) = action {
                ready.push((member.sequence, member.service.clone(), action));
            }
        }
        ready.sort_by_key(|(sequence, _, _)| *sequence);
        Ok(ready
            .into_iter()
            .take(limit)
            .map(|(_, service, action)| (service, action))
            .collect())
    }
}

fn dependencies_settled(
    context: &GraphExecutionContext,
    service: &str,
) -> Result<bool, GraphExecutionError> {
    context
        .dependencies
        .iter()
        .filter(|dependency| dependency.dependent == service)
        .try_fold(true, |settled, dependency| {
            if !settled {
                return Ok(false);
            }
            let target = context.members.get(&dependency.target).ok_or_else(|| {
                GraphExecutionError::MissingMember {
                    context_id: context.id,
                    service: dependency.target.clone(),
                }
            })?;
            Ok(match dependency.kind {
                ServiceDependencyKind::Requires | ServiceDependencyKind::BindsTo => {
                    target.status == GraphMemberStatus::Satisfied
                }
                ServiceDependencyKind::Wants => target.status.is_terminal(),
            })
        })
}
