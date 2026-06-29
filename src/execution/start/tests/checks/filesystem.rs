use crate::execution::graph::GraphExecutionStore;
use crate::execution::start::{
    PreStartCheckCompletionContext, RestartStartExecutionOutcome, StartExecutionOutcome,
    StartExecutionStore, begin_ready_start, begin_restart_start_leg,
    complete_pre_start_check_helper,
};
use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::OperationState;
use crate::operation::store::OperationStore;
use crate::service::runtime::ServiceState;
use crate::service::{ServiceCheck, ServiceCheckKind, ServiceTable};

use super::super::{operation_id, request_admin_start, service, start_request};
use super::support::{
    on_demand_dispatch, record_running_helper, report, restart_request, running_restart_operation,
};

#[test]
fn filesystem_condition_records_pending_graph_check_without_entering_starting() {
    let mut definition = service("app");
    definition.conditions = vec![ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/app".to_string(),
    }];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = OperationStore::new();
    let operation_id = operation_id(0);
    let request_outcome = request_admin_start(&mut operations, operation_id, "app");
    let mut graph = GraphExecutionStore::new();
    let context_id = graph
        .create_on_demand_context(&on_demand_dispatch("app", request_outcome), &services)
        .expect("graph context");
    let ready = graph
        .release_ready(context_id, 10)
        .expect("ready start")
        .remove(0);
    let mut jobs = JobStore::new();
    let mut job_ids = JobIdAllocator::new();
    let mut start_store = StartExecutionStore::new();

    let outcome = begin_ready_start(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        start_request(ready),
    )
    .expect("pending filesystem check");

    let StartExecutionOutcome::CheckPending(dispatch) = outcome else {
        panic!("expected pending filesystem check");
    };
    assert_eq!(dispatch.service, "app");
    assert_eq!(dispatch.operation_id, operation_id);
    assert_eq!(
        dispatch.helper_cgroup_id,
        "/sys/fs/cgroup/peinit/app/checks"
    );
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Inactive
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Running
    );
    assert_eq!(jobs.active_for_service("app"), Vec::new());
    assert_eq!(job_ids.next_sequence(), 0);
    assert_eq!(
        start_store.pending_pre_start_check_launches(),
        vec![operation_id]
    );
}

#[test]
fn filesystem_assert_records_pending_restart_check_after_entering_starting() {
    let mut definition = service("app");
    definition.asserts = vec![ServiceCheck {
        kind: ServiceCheckKind::Directory,
        argument: "/srv/app".to_string(),
    }];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = running_restart_operation("app");
    let operation_id = operation_id(0);
    let mut jobs = JobStore::new();
    let mut job_ids = JobIdAllocator::new();
    let mut start_store = StartExecutionStore::new();

    let outcome = begin_restart_start_leg(
        &mut services,
        &mut operations,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        restart_request("app", operation_id),
    )
    .expect("pending restart filesystem check");

    let RestartStartExecutionOutcome::CheckPending(dispatch) = outcome else {
        panic!("expected pending restart filesystem check");
    };
    assert_eq!(dispatch.service, "app");
    assert_eq!(dispatch.operation_id, operation_id);
    assert_eq!(
        dispatch.helper_cgroup_id,
        "/sys/fs/cgroup/peinit/app/checks"
    );
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Starting
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Running
    );
    assert_eq!(jobs.active_for_service("app"), Vec::new());
    assert_eq!(job_ids.next_sequence(), 0);
    assert_eq!(
        start_store.pending_pre_start_check_launches(),
        vec![operation_id]
    );
}

#[test]
fn passing_filesystem_condition_continues_graph_start_to_created_job() {
    let check = ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/app".to_string(),
    };
    let mut definition = service("app");
    definition.conditions = vec![check.clone()];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = OperationStore::new();
    let operation_id = operation_id(0);
    let request_outcome = request_admin_start(&mut operations, operation_id, "app");
    let mut graph = GraphExecutionStore::new();
    let context_id = graph
        .create_on_demand_context(&on_demand_dispatch("app", request_outcome), &services)
        .expect("graph context");
    let ready = graph
        .release_ready(context_id, 10)
        .expect("ready start")
        .remove(0);
    let mut jobs = JobStore::new();
    let mut job_ids = JobIdAllocator::new();
    let mut start_store = StartExecutionStore::new();
    begin_ready_start(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        start_request(ready),
    )
    .expect("pending filesystem check");
    record_running_helper(&mut start_store, "app", operation_id, vec![check.clone()]);

    let dispatch = complete_pre_start_check_helper(
        &mut PreStartCheckCompletionContext {
            services: &mut services,
            operations: &mut operations,
            graph: &mut graph,
            jobs: &mut jobs,
            job_ids: &mut job_ids,
            start_store: &mut start_store,
        },
        81,
        report("app", operation_id, vec![(check, true)]),
    )
    .expect("complete helper");

    assert!(dispatch.job_id.is_some());
    assert!(dispatch.job_event.is_some());
    assert_eq!(dispatch.service_transitions.len(), 1);
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Starting
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Running
    );
    assert_eq!(
        jobs.active_for_service("app"),
        vec![dispatch.job_id.unwrap()]
    );
}

#[test]
fn failing_filesystem_condition_skips_graph_start() {
    let check = ServiceCheck {
        kind: ServiceCheckKind::Directory,
        argument: "/srv/app".to_string(),
    };
    let mut definition = service("app");
    definition.conditions = vec![check.clone()];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = OperationStore::new();
    let operation_id = operation_id(0);
    let request_outcome = request_admin_start(&mut operations, operation_id, "app");
    let mut graph = GraphExecutionStore::new();
    let context_id = graph
        .create_on_demand_context(&on_demand_dispatch("app", request_outcome), &services)
        .expect("graph context");
    let ready = graph
        .release_ready(context_id, 10)
        .expect("ready start")
        .remove(0);
    let mut jobs = JobStore::new();
    let mut job_ids = JobIdAllocator::new();
    let mut start_store = StartExecutionStore::new();
    begin_ready_start(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        start_request(ready),
    )
    .expect("pending filesystem check");
    record_running_helper(&mut start_store, "app", operation_id, vec![check.clone()]);

    let dispatch = complete_pre_start_check_helper(
        &mut PreStartCheckCompletionContext {
            services: &mut services,
            operations: &mut operations,
            graph: &mut graph,
            jobs: &mut jobs,
            job_ids: &mut job_ids,
            start_store: &mut start_store,
        },
        81,
        report("app", operation_id, vec![(check, false)]),
    )
    .expect("complete helper");

    assert!(dispatch.job_id.is_none());
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Skipped
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Completed
    );
    assert_eq!(dispatch.graph_events.len(), 1);
    assert_eq!(jobs.active_for_service("app"), Vec::new());
}
