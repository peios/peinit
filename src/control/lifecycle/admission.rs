use crate::operation::OperationSource;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::{OperationRequest, OperationStore, OperationStoreError};
use crate::service::{ServiceTable, ServiceTableError};

use super::matrix::{CommandAdmission, OperationExpectation, classify};
use super::model::{
    LifecycleCommand, LifecycleCommandError, LifecycleCommandOutcome, LifecycleCommandRequest,
    ServiceStatusSnapshot,
};
use super::synchronous_clear::admit_synchronous_clear;

pub fn admit_lifecycle_command(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
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
        CommandAdmission::Operation { expectation } => {
            admit_operation(operations, request, expectation)
        }
        CommandAdmission::SynchronousClear { cause, result } => {
            admit_synchronous_clear(services, operations, request, cause, result)
        }
    }
}

pub(super) fn admit_operation(
    operations: &mut OperationStore,
    request: LifecycleCommandRequest,
    expectation: OperationExpectation,
) -> Result<LifecycleCommandOutcome, LifecycleCommandError> {
    let mut next_operations = operations.clone();
    let command = request.command;
    let service = request.service.clone();
    let outcome = match deferred_restart_to_merge_into(&next_operations, &service, expectation) {
        // The conflict table answers Restart × Restart with a queue, which is
        // right while one is running: the second waits its turn. A deferred
        // restart is not running, and a second one queued behind it would sit
        // Pending until the pending-operation timeout failed it. Merge, as a
        // second deferred start does (§10.3).
        Some(existing_id) => next_operations
            .merge_request_into(operation_request(request), existing_id)
            .map_err(LifecycleCommandError::OperationStore)?,
        None => next_operations
            .request_operation(operation_request(request))
            .map_err(LifecycleCommandError::OperationStore)?,
    };
    validate_expectation(&outcome.decision, expectation, &service, command)?;
    *operations = next_operations;
    Ok(LifecycleCommandOutcome::OperationAccepted(outcome))
}

fn deferred_restart_to_merge_into(
    operations: &OperationStore,
    service: &str,
    expectation: OperationExpectation,
) -> Option<crate::ids::OperationId> {
    if expectation != OperationExpectation::DeferredRestart {
        return None;
    }
    let existing = operations.current_for_service(service)?;
    (existing.operation_type == crate::operation::OperationType::Restart
        && existing.state == crate::operation::OperationState::Pending)
        .then_some(existing.id)
}

pub(super) fn operation_request(request: LifecycleCommandRequest) -> OperationRequest {
    OperationRequest {
        id: request.id,
        operation_type: request.command.operation_type(),
        service: request.service,
        source: OperationSource::Admin,
        caller: request.caller,
        created_at_ns: request.created_at_ns,
    }
}

pub(super) fn status_for(
    services: &ServiceTable,
    service: &str,
) -> Result<ServiceStatusSnapshot, LifecycleCommandError> {
    let entry = services
        .get(service)
        .ok_or_else(|| LifecycleCommandError::UnknownService {
            service: service.to_string(),
        })?;
    Ok(ServiceStatusSnapshot {
        service: service.to_string(),
        state: entry.runtime.state,
        cause: entry.runtime.cause,
        generation: entry.runtime.generation,
        definition_removed: entry.definition_removed,
    })
}

pub(super) fn rejects_definition_removed(command: LifecycleCommand) -> bool {
    matches!(
        command,
        LifecycleCommand::Start | LifecycleCommand::Restart | LifecycleCommand::Reload
    )
}

fn validate_expectation(
    decision: &OperationConflictDecision,
    expectation: OperationExpectation,
    service: &str,
    command: LifecycleCommand,
) -> Result<(), LifecycleCommandError> {
    match expectation {
        OperationExpectation::Any => Ok(()),
        OperationExpectation::DeferredStart => {
            if matches!(
                decision,
                OperationConflictDecision::CreateNew
                    | OperationConflictDecision::MergeIntoExisting { .. }
            ) {
                Ok(())
            } else {
                Err(LifecycleCommandError::ExpectedMerge {
                    service: service.to_string(),
                    command,
                })
            }
        }
        OperationExpectation::DeferredRestart => {
            // Nothing pending: the restart is created and waits for the
            // deadline. A deferred start pending: cancelled, `superseded_by_
            // restart`, and the restart takes its place. A deferred restart
            // pending: merged into, above.
            if matches!(
                decision,
                OperationConflictDecision::CreateNew
                    | OperationConflictDecision::CancelExistingThenQueue { .. }
                    | OperationConflictDecision::MergeIntoExisting { .. }
            ) {
                Ok(())
            } else {
                Err(LifecycleCommandError::ExpectedMerge {
                    service: service.to_string(),
                    command,
                })
            }
        }
        OperationExpectation::Merge => {
            if matches!(
                decision,
                OperationConflictDecision::MergeIntoExisting { .. }
            ) {
                Ok(())
            } else {
                Err(LifecycleCommandError::ExpectedMerge {
                    service: service.to_string(),
                    command,
                })
            }
        }
        OperationExpectation::Queue => {
            if matches!(
                decision,
                OperationConflictDecision::QueueNew
                    | OperationConflictDecision::CancelExistingThenQueue { .. }
            ) {
                Ok(())
            } else {
                Err(LifecycleCommandError::ExpectedQueue {
                    service: service.to_string(),
                    command,
                })
            }
        }
    }
}

impl From<ServiceTableError> for LifecycleCommandError {
    fn from(error: ServiceTableError) -> Self {
        Self::ServiceTable(error)
    }
}

impl From<OperationStoreError> for LifecycleCommandError {
    fn from(error: OperationStoreError) -> Self {
        Self::OperationStore(error)
    }
}
