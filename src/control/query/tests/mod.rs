mod operation;
mod service;

use crate::ids::{JobId, JobIdAllocator, OperationId, OperationIdAllocator};
use crate::job::{JobRecord, JobStore, ProcessHandle, ServiceMainJobSpec};
use crate::operation::store::OperationRequest;
use crate::operation::{OperationSource, OperationType};
use crate::security::TokenSummary;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTable};

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

fn operation_ids(count: usize) -> Vec<OperationId> {
    OperationIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("operation ids")
}

fn job_ids(count: usize) -> Vec<JobId> {
    JobIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("job ids")
}

fn service(name: &str, image_path: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, image_path)
}

fn service_table(names: &[&str]) -> ServiceTable {
    ServiceTable::from_boot_snapshot(
        names
            .iter()
            .map(|name| service(name, &format!("/sbin/{name}")))
            .collect(),
    )
    .expect("service table")
}

fn transition_to(table: &mut ServiceTable, service: &str, state: ServiceState) {
    match state {
        ServiceState::Inactive => {}
        ServiceState::Starting => {
            table
                .transition_service(
                    service,
                    ServiceTransition {
                        to: ServiceState::Starting,
                        cause: TransitionCause::ExplicitStart,
                    },
                )
                .expect("starting");
        }
        ServiceState::Active => {
            transition_to(table, service, ServiceState::Starting);
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
        ServiceState::Failed => {
            table
                .transition_service(
                    service,
                    ServiceTransition {
                        to: ServiceState::Failed,
                        cause: TransitionCause::ValidationError,
                    },
                )
                .expect("failed");
        }
        _ => panic!("test helper does not build this state"),
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

fn create_started_job(
    jobs: &mut JobStore,
    id: JobId,
    operation_id: OperationId,
    service: &ServiceDefinition,
) {
    let job = JobRecord::new_service_main(
        id,
        ServiceMainJobSpec {
            service,
            resolved_identity: service.identity.clone(),
            token_summary: TokenSummary::requested_identity(service.identity.clone()),
            activation_generation: 0,
            cgroup_generation: 0,
            operation_id,
            created_at_ns: 1_000,
        },
    );
    jobs.create_job(job).expect("create job");
    jobs.start_job(
        id,
        ProcessHandle {
            pid: 4242,
            pidfd: 9,
        },
        1_010,
    )
    .expect("start job");
}
