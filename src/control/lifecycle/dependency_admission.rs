use crate::ids::OperationIdAllocator;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;

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
            if should_plan_on_demand_start(request.command, expectation) =>
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

fn should_plan_on_demand_start(
    command: LifecycleCommand,
    expectation: OperationExpectation,
) -> bool {
    command == LifecycleCommand::Start && expectation == OperationExpectation::Any
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
