mod cgroup;
mod lifecycle;
mod store;

use crate::ids::{JobId, JobIdAllocator, OperationId, OperationIdAllocator};
use crate::job::{JobRecord, ServiceMainJobSpec};
use crate::security::TokenSummary;
use crate::service::ServiceDefinition;

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

fn token_summary(identity: &str) -> TokenSummary {
    TokenSummary::requested_identity(identity)
}

fn job_ids(count: usize) -> Vec<JobId> {
    JobIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("job ids")
}

fn operation_ids(count: usize) -> Vec<OperationId> {
    OperationIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("operation ids")
}

fn service() -> ServiceDefinition {
    let mut service = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    service.arguments = vec!["--foreground".to_string(), "--ready".to_string()];
    service.identity = "LocalService".to_string();
    service
}

fn service_main_job() -> JobRecord {
    JobRecord::new_service_main(
        job_ids(1)[0],
        ServiceMainJobSpec {
            service: &service(),
            resolved_identity: "LocalService".to_string(),
            token_summary: token_summary("LocalService"),
            activation_generation: 0,
            cgroup_generation: 0,
            operation_id: operation_ids(1)[0],
            created_at_ns: 1_000,
        },
    )
}
