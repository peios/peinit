mod blocked;
mod dispatch;
mod success;

use crate::control::lifecycle::PlannedStart;
use crate::ids::{OperationId, OperationIdAllocator};
use crate::operation::OperationSource;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTable};

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

fn service(name: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"))
}

fn table(definitions: Vec<ServiceDefinition>) -> ServiceTable {
    ServiceTable::from_boot_snapshot(definitions).expect("service table")
}

fn planned_services(plan: &[PlannedStart]) -> Vec<(&str, OperationSource, TransitionCause)> {
    plan.iter()
        .map(|start| {
            (
                start.service.as_str(),
                start.operation_source,
                start.transition_cause,
            )
        })
        .collect()
}

fn activate(table: &mut ServiceTable, service: &str) {
    table
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("starting");
    table
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Active,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("active");
}

fn operation_ids(count: usize) -> Vec<OperationId> {
    OperationIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("operation ids")
}
