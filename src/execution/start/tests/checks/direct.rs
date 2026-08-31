use crate::execution::graph::{GraphExecutionStore, GraphTerminalOutcome};
use crate::execution::start::{
    StartExecutionOutcome, StartExecutionStore, StartPreCheckTerminalOutcome, begin_ready_start,
};
use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::OperationState;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::super::{operation_id, request_admin_start, service, start_request};
use super::support::{on_demand_dispatch, registry_check};

#[test]
fn failing_registry_condition_skips_service_and_satisfies_graph() {
    let mut definition = service("app");
    definition.conditions = vec![registry_check("Machine\\System\\Services\\missing")];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = OperationStore::new();
    let operation_id = operation_id(0);
    let request_outcome = request_admin_start(&mut operations, operation_id, "app");
    let mut graph = GraphExecutionStore::new();
    let context_id = graph
        .create_on_demand_context(&on_demand_dispatch("app", request_outcome), &services)
        .expect("graph context");
    let ready = graph
        .release_ready(context_id, 10, &|_, _| {
            crate::execution::graph::LevelProbe::Absent
        })
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
    .expect("condition skip");

    let StartExecutionOutcome::Terminal(dispatch) = outcome else {
        panic!("expected terminal condition outcome");
    };
    assert_eq!(
        dispatch.outcome,
        StartPreCheckTerminalOutcome::ConditionSkipped {
            check: "registry:Machine\\System\\Services\\missing".to_string(),
        }
    );
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Skipped
    );
    assert_eq!(
        services.runtime("app").expect("runtime").cause,
        Some(TransitionCause::ConditionSkipped)
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Completed
    );
    assert_eq!(jobs.active_for_service("app"), Vec::new());
    assert_eq!(job_ids.next_sequence(), 0);
    assert_eq!(
        dispatch.graph_events[0].outcome,
        GraphTerminalOutcome::Satisfied
    );
}

#[test]
fn failing_registry_assert_fails_service_and_graph() {
    let mut definition = service("app");
    definition.asserts = vec![registry_check("Machine\\System\\Services\\missing")];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = OperationStore::new();
    let operation_id = operation_id(0);
    let request_outcome = request_admin_start(&mut operations, operation_id, "app");
    let mut graph = GraphExecutionStore::new();
    let context_id = graph
        .create_on_demand_context(&on_demand_dispatch("app", request_outcome), &services)
        .expect("graph context");
    let ready = graph
        .release_ready(context_id, 10, &|_, _| {
            crate::execution::graph::LevelProbe::Absent
        })
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
    .expect("assert failure");

    let StartExecutionOutcome::Terminal(dispatch) = outcome else {
        panic!("expected terminal assert outcome");
    };
    assert_eq!(
        dispatch.outcome,
        StartPreCheckTerminalOutcome::AssertionFailed {
            check: "registry:Machine\\System\\Services\\missing".to_string(),
        }
    );
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Failed
    );
    assert_eq!(
        services.runtime("app").expect("runtime").cause,
        Some(TransitionCause::AssertionError)
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Failed
    );
    assert_eq!(jobs.active_for_service("app"), Vec::new());
    assert_eq!(job_ids.next_sequence(), 0);
    assert_eq!(
        dispatch.graph_events[0].outcome,
        GraphTerminalOutcome::Failed
    );
}
