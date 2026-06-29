use crate::boot::BootMode;
use crate::boot::phase2::prepare_phase2_boot_plan;
use crate::execution::graph::GraphExecutionStore;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::job::{JobEventDetail, JobState, JobStore};
use crate::operation::OperationState;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::{OBSERVED_AT_NS, service, start_request};
use crate::execution::start::{StartExecutionOutcome, StartExecutionStore, begin_ready_start};

#[test]
fn boot_ready_start_uses_reserved_job_id_and_begins_runtime_records() {
    let definition = service("app");
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        std::slice::from_ref(&definition),
        10,
        OBSERVED_AT_NS,
        &mut operation_ids,
        &mut job_ids,
    )
    .expect("boot plan");
    let reserved_job_id = plan.starts[0].job_id;
    let operation_id = plan.starts[0].operation_id;
    let mut services =
        ServiceTable::from_boot_snapshot(vec![definition.clone()]).expect("service table");
    let mut operations = OperationStore::new();
    operations
        .dispatch_phase2_boot_plan(&plan)
        .expect("dispatch operations");
    let mut jobs = JobStore::new();
    let mut start_store = StartExecutionStore::new();
    let mut graph = GraphExecutionStore::new();
    let context_id = graph
        .create_boot_context(&plan, &[definition])
        .expect("graph context");
    let ready = graph
        .release_ready(context_id, 10)
        .expect("ready starts")
        .remove(0);

    let dispatch = begin_ready_start(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        start_request(ready),
    )
    .expect("begin start");
    let StartExecutionOutcome::Job(dispatch) = dispatch else {
        panic!("expected created start job");
    };

    assert_eq!(dispatch.job_id, reserved_job_id);
    assert_eq!(job_ids.next_sequence(), 1);
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Starting
    );
    assert_eq!(
        services.runtime("app").expect("runtime").cause,
        Some(TransitionCause::ExplicitStart)
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Running
    );
    let job = jobs.get(reserved_job_id).expect("job");
    assert_eq!(job.state, JobState::Created);
    assert_eq!(job.operation_id, Some(operation_id));
    assert_eq!(job.activation_generation, 1);
    assert_eq!(job.cgroup_generation, 0);
    assert_eq!(job.cgroup_id, "/sys/fs/cgroup/peinit/app/main");
    assert_eq!(
        dispatch.job_event.detail,
        JobEventDetail::Created {
            image_path: "/sbin/app".to_string(),
            identity: "SYSTEM".to_string(),
            operation_id: Some(operation_id),
        }
    );
}
