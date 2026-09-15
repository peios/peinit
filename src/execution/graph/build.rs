use std::collections::{BTreeMap, BTreeSet};

use crate::boot::phase2::Phase2BootPlan;
use crate::boot::phase2::StartCause;
use crate::control::lifecycle::OnDemandStartDispatch;
use crate::operation::conflict::OperationConflictDecision;
use crate::service::runtime::TransitionCause;
use crate::service::{ServiceDefinition, ServiceTable};

use super::dependencies::{context_dependencies, definitions_by_name};
use super::model::{
    GraphContextBuildError, GraphContextId, GraphContextKind, GraphExecutionContext, GraphMember,
    GraphMemberStatus,
};

pub(super) fn boot_context(
    id: GraphContextId,
    plan: &Phase2BootPlan,
    definitions: &[ServiceDefinition],
) -> Result<GraphExecutionContext, GraphContextBuildError> {
    let mut members = BTreeMap::new();
    for (sequence, start) in plan.starts.iter().enumerate() {
        insert_member(
            &mut members,
            GraphMember {
                service: start.service.clone(),
                operation_id: start.operation_id,
                reserved_job_id: Some(start.job_id),
                transition_cause: transition_cause_for_phase2_start(start.cause),
                sequence,
                status: GraphMemberStatus::Dormant,
                held_without_clock: false,
            },
        )?;
    }
    for (offset, blocked) in plan.blocked.iter().enumerate() {
        insert_member(
            &mut members,
            GraphMember {
                service: blocked.service.clone(),
                operation_id: blocked.operation_id,
                reserved_job_id: None,
                transition_cause: blocked.reason.transition_cause(),
                sequence: plan.starts.len() + offset,
                status: GraphMemberStatus::Failed,
                held_without_clock: false,
            },
        )?;
    }

    let by_name = definitions_by_name(definitions);
    let dependencies = context_dependencies(&members, |service| by_name.get(service).copied())?;
    activate_roots(&mut members, &dependencies);
    Ok(GraphExecutionContext {
        id,
        kind: GraphContextKind::Boot,
        members,
        dependencies,
    })
}

pub(super) fn on_demand_context(
    id: GraphContextId,
    dispatch: &OnDemandStartDispatch,
    services: &ServiceTable,
) -> Result<GraphExecutionContext, GraphContextBuildError> {
    let mut members = BTreeMap::new();
    let mut dependency_outcomes = dispatch.dependency_operations.iter();

    for (sequence, start) in dispatch.plan.starts.iter().enumerate() {
        let outcome = if start.service == dispatch.plan.requested {
            &dispatch.requested_operation
        } else {
            dependency_outcomes.next().ok_or_else(|| {
                GraphContextBuildError::MissingDependencyOperationOutcome {
                    service: start.service.clone(),
                }
            })?
        };
        let status = if matches!(
            outcome.decision,
            OperationConflictDecision::MergeIntoExisting { .. }
        ) {
            GraphMemberStatus::Running
        } else {
            GraphMemberStatus::Dormant
        };
        insert_member(
            &mut members,
            GraphMember {
                service: start.service.clone(),
                operation_id: outcome.returned_operation_id,
                reserved_job_id: None,
                transition_cause: start.transition_cause,
                sequence,
                status,
                held_without_clock: false,
            },
        )?;
    }

    if members.is_empty() {
        insert_member(
            &mut members,
            GraphMember {
                service: dispatch.plan.requested.clone(),
                operation_id: dispatch.requested_operation.returned_operation_id,
                reserved_job_id: None,
                transition_cause: TransitionCause::DependencyFailure,
                sequence: 0,
                status: blocked_requested_status(dispatch),
                held_without_clock: false,
            },
        )?;
    }

    let dependencies = context_dependencies(&members, |service| services.definition(service))?;
    activate_roots(&mut members, &dependencies);
    Ok(GraphExecutionContext {
        id,
        kind: GraphContextKind::OnDemand {
            requested_service: dispatch.plan.requested.clone(),
            requested_operation_id: dispatch.requested_operation.returned_operation_id,
        },
        members,
        dependencies,
    })
}

fn activate_roots(
    members: &mut BTreeMap<String, GraphMember>,
    dependencies: &[super::model::GraphDependency],
) {
    let dependency_targets = dependencies
        .iter()
        .map(|dependency| dependency.target.as_str())
        .collect::<BTreeSet<_>>();
    for member in members.values_mut() {
        if member.status == GraphMemberStatus::Dormant
            && !dependency_targets.contains(member.service.as_str())
        {
            member.status = GraphMemberStatus::WaitingForPreStartCheck;
        }
    }
}

fn insert_member(
    members: &mut BTreeMap<String, GraphMember>,
    member: GraphMember,
) -> Result<(), GraphContextBuildError> {
    if members.contains_key(&member.service) {
        return Err(GraphContextBuildError::DuplicateServiceMember {
            service: member.service,
        });
    }
    members.insert(member.service.clone(), member);
    Ok(())
}

fn blocked_requested_status(dispatch: &OnDemandStartDispatch) -> GraphMemberStatus {
    if dispatch.requested_operation.returned_operation_id
        == dispatch.requested_operation.stored_operation_id
    {
        GraphMemberStatus::Failed
    } else {
        GraphMemberStatus::Running
    }
}

fn transition_cause_for_phase2_start(cause: StartCause) -> TransitionCause {
    match cause {
        StartCause::ExplicitStart => TransitionCause::ExplicitStart,
        StartCause::DependencyStart => TransitionCause::DependencyStart,
    }
}
