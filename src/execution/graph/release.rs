use crate::service::ServiceDependencyKind;

use super::model::{
    GraphContextId, GraphDependency, GraphExecutionContext, GraphExecutionError, GraphMemberStatus,
    LevelProbe, ReadyGraphOperation, ReadyGraphOperationAction,
};
use super::store::GraphExecutionStore;

impl GraphExecutionStore {
    pub fn release_ready(
        &mut self,
        context_id: GraphContextId,
        max_parallel_starts: u32,
        probe: &dyn Fn(&str, &str) -> LevelProbe,
    ) -> Result<Vec<ReadyGraphOperation>, GraphExecutionError> {
        if max_parallel_starts == 0 {
            return Err(GraphExecutionError::InvalidMaxParallelStarts);
        }
        let available_slots = self.available_slots(context_id, max_parallel_starts)?;
        let ready = self.ready_members(context_id, available_slots, probe)?;
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
                // What the last pass found while it still waited: a hold
                // with no clock ends here, and the lifetime starts here.
                released_from_hold: member.held_without_clock,
            });
        }
        // What the members still waiting are held on, for the pass that
        // releases them.
        context.note_holds_without_clock();

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
        probe: &dyn Fn(&str, &str) -> LevelProbe,
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
                    if dependencies_settled(context, &member.service, probe)? =>
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
    probe: &dyn Fn(&str, &str) -> LevelProbe,
) -> Result<bool, GraphExecutionError> {
    context
        .dependencies
        .iter()
        .filter(|dependency| dependency.dependent == service)
        .try_fold(true, |settled, dependency| {
            if !settled {
                return Ok(false);
            }
            dependency_settled(context, dependency, probe)
        })
}

/// Is this one edge settled, so the dependent may proceed past it?
///
/// A level edge is settled by a *live* fact, not a recorded one: the level
/// can arrive after the target's start operation completed (netd reaches
/// Active well before DHCP finishes) and can be retracted while a dependent
/// is still waiting. Member completion alone therefore answers only the
/// level-less edges.
fn dependency_settled(
    context: &GraphExecutionContext,
    dependency: &GraphDependency,
    probe: &dyn Fn(&str, &str) -> LevelProbe,
) -> Result<bool, GraphExecutionError> {
    let member = context.members.get(&dependency.target);
    let Some(level) = dependency.level.as_deref() else {
        // A level-less edge always has a member target; the build never
        // emits one otherwise.
        let target = member.ok_or_else(|| GraphExecutionError::MissingMember {
            context_id: context.id,
            service: dependency.target.clone(),
        })?;
        return Ok(match dependency.kind {
            ServiceDependencyKind::Requires | ServiceDependencyKind::BindsTo => {
                target.status == GraphMemberStatus::Satisfied
            }
            ServiceDependencyKind::Wants => target.status.is_terminal(),
        });
    };

    let probed = probe(&dependency.target, level);
    Ok(match dependency.kind {
        // The hard gate: nothing but the level itself opens it. A target
        // that is a member must additionally have finished its own start —
        // a `LEVEL=` sent while the start operation is still in flight
        // must not release the dependent ahead of the ordering edge.
        ServiceDependencyKind::Requires | ServiceDependencyKind::BindsTo => {
            member.is_none_or(|target| target.status == GraphMemberStatus::Satisfied)
                && probed == LevelProbe::Satisfied
        }
        // The soft gate waits only while someone could still publish the
        // level: a running target holds it, a dead or absent one does not.
        // That keeps `Wants` failure-tolerant — the property that defines
        // it — while still giving "wait for it if it is coming" semantics.
        ServiceDependencyKind::Wants => {
            let member_settled = member.is_none_or(|target| target.status.is_terminal());
            member_settled && probed != LevelProbe::NotYetPublished
        }
    })
}
