use crate::ids::OperationIdAllocator;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::ServiceState;

use super::admission::{admit_operation, rejects_definition_removed, status_for};
use super::matrix::{CommandAdmission, OperationExpectation, classify};
use super::model::{
    LifecycleCommand, LifecycleCommandError, LifecycleCommandOutcome, LifecycleCommandRequest,
};
use super::start_plan::{dispatch_on_demand_start_plan, plan_on_demand_start};
use super::synchronous_clear::admit_synchronous_clear;

pub fn admit_lifecycle_command_with_operation_ids(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    operation_ids: &mut OperationIdAllocator,
    request: LifecycleCommandRequest,
) -> Result<LifecycleCommandOutcome, LifecycleCommandError> {
    let status = status_for(services, &request.service)?;
    if status.definition_removed && rejects_definition_removed(request.command) {
        return Err(LifecycleCommandError::DefinitionRemoved {
            service: request.service,
        });
    }

    match classify(request.command, status.state) {
        CommandAdmission::Already => Ok(LifecycleCommandOutcome::Already(status)),
        CommandAdmission::Noop => Ok(LifecycleCommandOutcome::Noop(status)),
        CommandAdmission::Invalid => Err(LifecycleCommandError::InvalidState {
            service: request.service,
            command: request.command,
            state: status.state,
        }),
        CommandAdmission::Operation { expectation }
            if should_plan_on_demand_start(request.command, expectation, status.state) =>
        {
            admit_on_demand_start(services, operations, operation_ids, request)
        }
        CommandAdmission::Operation { expectation } => {
            admit_operation(operations, request, expectation)
        }
        CommandAdmission::SynchronousClear { cause, result } => {
            admit_synchronous_clear(services, operations, request, cause, result)
        }
    }
}

/// Whether this command starts a service that is not running.
///
/// §8.1: "If the service has no running process (Inactive, Completed, Failed,
/// Skipped), the stop phase is skipped and peinit proceeds directly to the
/// start phase. **The operation type remains Restart for observability.**"
///
/// Only `Start` was routed here, so a `Restart` from one of those four states
/// went to the control boundary, where `process_target` unconditionally looks
/// for a live main job and errors with `MissingCurrentMainJob` or
/// `JobNotRunning`. That is four of the ten states — and they are the states an
/// operator most often restarts from: a service that failed, a Oneshot that
/// completed, a service somebody stopped earlier. An operator scripting a
/// restart across a set of services got an error for every one that happened
/// not to be running.
///
/// `Backoff` is deliberately not here either, but not because the stop phase
/// has something to do — there is no process, and no `Backoff -> Stopping`
/// edge. A restart in Backoff is admitted as a deferred restart
/// (`OperationExpectation::DeferredRestart`): it replaces the automatic
/// restart as the operation the backoff deadline executes, and never reaches
/// the control boundary at all (PEI-803).
fn should_plan_on_demand_start(
    command: LifecycleCommand,
    expectation: OperationExpectation,
    state: ServiceState,
) -> bool {
    if expectation != OperationExpectation::Any {
        return false;
    }
    match command {
        LifecycleCommand::Start => true,
        LifecycleCommand::Restart => matches!(
            state,
            ServiceState::Inactive
                | ServiceState::Completed
                | ServiceState::Failed
                | ServiceState::Skipped
        ),
        _ => false,
    }
}

fn admit_on_demand_start(
    services: &ServiceTable,
    operations: &mut OperationStore,
    operation_ids: &mut OperationIdAllocator,
    request: LifecycleCommandRequest,
) -> Result<LifecycleCommandOutcome, LifecycleCommandError> {
    let plan = plan_on_demand_start(services, &request.service)
        .map_err(LifecycleCommandError::StartPlan)?;
    let dispatch = dispatch_on_demand_start_plan(
        operations,
        operation_ids,
        plan,
        request.id,
        request.caller,
        request.created_at_ns,
    )
    .map_err(LifecycleCommandError::StartDispatch)?;
    Ok(LifecycleCommandOutcome::OnDemandStart(dispatch))
}
