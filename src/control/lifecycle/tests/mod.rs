mod dependency_admission;
mod matrix;
mod start_plan;
mod synchronous;

use crate::control::lifecycle::{LifecycleCommand, LifecycleCommandRequest};
use crate::ids::{OperationId, OperationIdAllocator};
use crate::operation::store::OperationRequest;
use crate::operation::{OperationSource, OperationType};
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTable};

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

fn ids(count: usize) -> Vec<OperationId> {
    OperationIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("operation ids")
}

fn command_request(
    id: OperationId,
    command: LifecycleCommand,
    service: &str,
    created_at_ns: u64,
) -> LifecycleCommandRequest {
    LifecycleCommandRequest {
        id,
        command,
        service: service.to_string(),
        caller: None,
        created_at_ns,
    }
}

fn operation_request(
    id: OperationId,
    operation_type: OperationType,
    service: &str,
    created_at_ns: u64,
) -> OperationRequest {
    OperationRequest {
        id,
        operation_type,
        service: service.to_string(),
        source: OperationSource::Admin,
        caller: None,
        created_at_ns,
    }
}

fn service_table() -> ServiceTable {
    ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "svc",
        "/sbin/svc",
    )])
    .expect("service table")
}

fn transition_to(table: &mut ServiceTable, state: ServiceState) {
    match state {
        ServiceState::Inactive => {}
        ServiceState::Starting => {
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Starting,
                        cause: TransitionCause::ExplicitStart,
                    },
                )
                .expect("starting");
        }
        ServiceState::Active => {
            transition_to(table, ServiceState::Starting);
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Active,
                        cause: TransitionCause::ExplicitStart,
                    },
                )
                .expect("active");
        }
        ServiceState::Reloading => {
            transition_to(table, ServiceState::Active);
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Reloading,
                        cause: TransitionCause::ExplicitReload,
                    },
                )
                .expect("reloading");
        }
        ServiceState::Stopping => {
            transition_to(table, ServiceState::Active);
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Stopping,
                        cause: TransitionCause::ExplicitStop,
                    },
                )
                .expect("stopping");
        }
        ServiceState::Completed => {
            transition_to(table, ServiceState::Starting);
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Completed,
                        cause: TransitionCause::ExplicitStart,
                    },
                )
                .expect("completed");
        }
        ServiceState::Backoff => {
            transition_to(table, ServiceState::Starting);
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Backoff,
                        cause: TransitionCause::ReadinessTimeout,
                    },
                )
                .expect("backoff");
        }
        ServiceState::Failed => {
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Failed,
                        cause: TransitionCause::ValidationError,
                    },
                )
                .expect("failed");
        }
        ServiceState::Skipped => {
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Skipped,
                        cause: TransitionCause::ConditionSkipped,
                    },
                )
                .expect("skipped");
        }
        ServiceState::Abandoned => {
            transition_to(table, ServiceState::Stopping);
            table
                .transition_service(
                    "svc",
                    ServiceTransition {
                        to: ServiceState::Abandoned,
                        cause: TransitionCause::ProcessUnkillable,
                    },
                )
                .expect("abandoned");
        }
    }
}
