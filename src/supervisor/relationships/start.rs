use std::collections::BTreeSet;

use crate::control::lifecycle::{
    LifecycleCommandError, dispatch_on_demand_start_plan, plan_binds_to_recovery_start,
    plan_on_failure_start, plan_tty_release_start, start_is_already_satisfied,
};
use crate::execution::start::StartExecutionDispatch;
use crate::ids::OperationId;
use crate::operation::OperationSource;
use crate::security::TokenSummary;

use super::conflict::gate_start_context;
use super::model::OnFailureChain;
use super::operation::allocate_operation_id;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

pub(super) fn dispatch_binds_to_recovery_start(
    work: &mut SupervisorWork,
    service: &str,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    dispatch_relationship_start(
        work,
        RelationshipStart {
            service,
            source: OperationSource::BindsToRecovery,
            plan: RelationshipStartPlan::BindsToRecovery,
            on_failure_chain: None,
        },
        observed_at_ns,
        max_parallel_starts,
    )
}

pub(super) fn dispatch_on_failure_start(
    work: &mut SupervisorWork,
    service: &str,
    chain: OnFailureChain,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    dispatch_relationship_start(
        work,
        RelationshipStart {
            service,
            source: OperationSource::OnFailure,
            plan: RelationshipStartPlan::OnFailure,
            on_failure_chain: Some(chain),
        },
        observed_at_ns,
        max_parallel_starts,
    )
}

pub(super) fn dispatch_tty_release_start(
    work: &mut SupervisorWork,
    service: &str,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    dispatch_relationship_start(
        work,
        RelationshipStart {
            service,
            source: OperationSource::TtyRelease,
            plan: RelationshipStartPlan::TtyRelease,
            on_failure_chain: None,
        },
        observed_at_ns,
        max_parallel_starts,
    )
}

fn dispatch_relationship_start(
    work: &mut SupervisorWork,
    request: RelationshipStart<'_>,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    // A relationship start names a service the supervisor picked, not one an
    // administrator typed, so it never passes through `admit_lifecycle_command`
    // and never gets that path's "already active -> no-op" answer. Ask the same
    // question here. Without this an `OnFailure` handler that is already Active
    // is planned for a start it has no legal transition into, and the resulting
    // `InvalidTransition` surfaces out of the failure-reaction pipeline
    // (PEI-597).
    //
    // Resolved before the operation id is allocated: there is nothing to start,
    // so there should be no operation, no context, and no chain entry recording
    // a cascade that never happened.
    if work
        .services
        .get(request.service)
        .is_some_and(|entry| start_is_already_satisfied(entry.runtime.state))
    {
        return Ok(Vec::new());
    }

    let request_id = allocate_operation_id(work, observed_at_ns)?;
    let plan = match request.plan {
        RelationshipStartPlan::BindsToRecovery => {
            plan_binds_to_recovery_start(&work.services, request.service)
        }
        RelationshipStartPlan::OnFailure => plan_on_failure_start(&work.services, request.service),
        RelationshipStartPlan::TtyRelease => {
            plan_tty_release_start(&work.services, request.service)
        }
    }
    .map_err(|source| SupervisorError::Lifecycle(LifecycleCommandError::StartPlan(source)))?;
    let start_services = plan
        .starts
        .iter()
        .map(|start| start.service.clone())
        .collect::<BTreeSet<_>>();
    let outcome = dispatch_on_demand_start_plan(
        &mut work.operations,
        &mut work.operation_ids,
        plan,
        request_id,
        None::<TokenSummary>,
        observed_at_ns,
    )
    .map_err(|source| SupervisorError::Lifecycle(LifecycleCommandError::StartDispatch(source)))?;

    if request.source == OperationSource::OnFailure
        && outcome.requested_operation.returned_operation_id
            == outcome.requested_operation.stored_operation_id
        && operation_is_live(
            &work.operations,
            outcome.requested_operation.returned_operation_id,
        )
        && let Some(chain) = request.on_failure_chain
    {
        work.relationships
            .record_on_failure_chain(request.service.to_string(), chain);
    }

    let context_id = work
        .graph
        .create_on_demand_context(&outcome, &work.services)
        .map_err(SupervisorError::GraphContext)?;
    if start_services.is_empty() {
        return Ok(Vec::new());
    }
    gate_start_context(
        work,
        context_id,
        start_services,
        observed_at_ns,
        max_parallel_starts,
    )
}

fn operation_is_live(
    operations: &crate::operation::store::OperationStore,
    operation_id: OperationId,
) -> bool {
    operations
        .get(operation_id)
        .is_some_and(|operation| !operation.state.is_terminal())
}

struct RelationshipStart<'a> {
    service: &'a str,
    source: OperationSource,
    plan: RelationshipStartPlan,
    on_failure_chain: Option<OnFailureChain>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationshipStartPlan {
    BindsToRecovery,
    OnFailure,
    TtyRelease,
}
