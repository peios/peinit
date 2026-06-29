mod rollback;
mod success;
mod validation;

use crate::ids::{OperationId, OperationIdAllocator};
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTable};

const DUE_AT_NS: u64 = 2_000;
const NOW_NS: u64 = 3_000;

fn service(name: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"))
}

fn table(definitions: Vec<ServiceDefinition>) -> ServiceTable {
    ServiceTable::from_boot_snapshot(definitions).expect("service table")
}

fn put_active_service_in_backoff(table: &mut ServiceTable, service: &str, due_at_ns: u64) {
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
    table
        .transition_service_to_restart_backoff(service, TransitionCause::ProcessCrash, due_at_ns)
        .expect("backoff");
}

fn operation_ids_from_start(count: usize) -> Vec<OperationId> {
    OperationIdAllocator::new()
        .allocate_batch(count, NOW_NS)
        .expect("operation ids")
}
