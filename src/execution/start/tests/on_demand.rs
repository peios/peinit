use crate::execution::graph::GraphExecutionStore;
use crate::execution::start::begin_ready_start;
use crate::ids::JobIdAllocator;
use crate::job::{JobRecord, JobStore, ServiceMainJobSpec};
use crate::operation::OperationState;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::ServiceState;

use super::{
    OBSERVED_AT_NS, on_demand_dispatch, operation_id, request_admin_start, service, start_request,
    token_summary,
};
use crate::execution::start::{StartExecutionError, StartExecutionOutcome, StartExecutionStore};

#[test]
fn on_demand_ready_start_allocates_job_id_atomically() {
    let definition = service("app");
    let mut services =
        ServiceTable::from_boot_snapshot(vec![definition.clone()]).expect("service table");
    let mut operations = OperationStore::new();
    let operation_id = operation_id(0);
    let request_outcome = request_admin_start(&mut operations, operation_id, "app");
    let mut graph = GraphExecutionStore::new();
    let context_id = graph
        .create_on_demand_context(&on_demand_dispatch("app", request_outcome), &services)
        .expect("graph context");
    let ready = graph
        .release_ready(context_id, 10)
        .expect("ready starts")
        .remove(0);
    assert_eq!(ready.reserved_job_id, None);
    let mut jobs = JobStore::new();
    let mut job_ids = JobIdAllocator::new();
    let mut start_store = StartExecutionStore::new();

    let dispatch = begin_ready_start(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        start_request(ready),
    )
    .expect("begin on-demand start");
    let StartExecutionOutcome::Job(dispatch) = dispatch else {
        panic!("expected created start job");
    };

    assert_eq!(job_ids.next_sequence(), 1);
    assert_eq!(jobs.active_for_service("app"), vec![dispatch.job_id]);
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Running
    );
}

#[test]
fn failed_job_creation_rolls_back_operation_service_and_allocator() {
    let definition = service("app");
    let mut services =
        ServiceTable::from_boot_snapshot(vec![definition.clone()]).expect("service table");
    let mut operations = OperationStore::new();
    let operation_id = operation_id(0);
    let request_outcome = request_admin_start(&mut operations, operation_id, "app");
    let mut graph = GraphExecutionStore::new();
    let context_id = graph
        .create_on_demand_context(&on_demand_dispatch("app", request_outcome), &services)
        .expect("graph context");
    let ready = graph
        .release_ready(context_id, 10)
        .expect("ready starts")
        .remove(0);
    let mut job_ids = JobIdAllocator::new();
    let existing_job_id = job_ids
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("existing job id")[0];
    let mut jobs = JobStore::new();
    let mut start_store = StartExecutionStore::new();
    jobs.create_job(JobRecord::new_service_main(
        existing_job_id,
        ServiceMainJobSpec {
            service: &definition,
            resolved_identity: "SYSTEM".to_string(),
            token_summary: token_summary(),
            activation_generation: 1,
            cgroup_generation: 0,
            operation_id,
            created_at_ns: OBSERVED_AT_NS,
        },
    ))
    .expect("existing job");

    let err = begin_ready_start(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        start_request(ready),
    )
    .expect_err("second active service-main job");

    assert!(matches!(err, StartExecutionError::JobStore(_)));
    assert_eq!(job_ids.next_sequence(), 1);
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Inactive
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Pending
    );
    assert_eq!(jobs.active_for_service("app"), vec![existing_job_id]);
}
