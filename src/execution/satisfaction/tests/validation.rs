use crate::boot::BootMode;
use crate::boot::phase2::prepare_phase2_boot_plan;
use crate::execution::graph::GraphExecutionStore;
use crate::execution::satisfaction::{StartSatisfactionError, apply_start_satisfaction};
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::operation::OperationState;
use crate::operation::store::OperationStore;
use crate::service::{ServiceTable, ServiceType};

use crate::execution::test_support::{
    OBSERVED_AT_NS, StartedBootGraph, satisfaction_request, service,
};

#[test]
fn satisfaction_rejects_wrong_service_without_mutation() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();
    let operation_id = fixture.started_operation_id;

    let err = apply_start_satisfaction(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        satisfaction_request("other", operation_id),
    )
    .expect_err("service mismatch");

    assert_eq!(
        err,
        StartSatisfactionError::OperationServiceMismatch {
            operation_id,
            expected_service: "other".to_string(),
            actual_service: "app".to_string(),
        }
    );
    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
}

#[test]
fn satisfaction_rejects_pending_operation_without_mutation() {
    let app = service("app", ServiceType::Simple);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        std::slice::from_ref(&app),
        10,
        OBSERVED_AT_NS,
        &mut operation_ids,
        &mut job_ids,
    )
    .expect("boot plan");
    let operation_id = plan.starts[0].operation_id;
    let mut services = ServiceTable::from_boot_snapshot(vec![app.clone()]).expect("service table");
    let mut operations = OperationStore::new();
    operations
        .dispatch_phase2_boot_plan(&plan)
        .expect("operation dispatch");
    let mut graph = GraphExecutionStore::new();
    graph
        .create_boot_context(&plan, std::slice::from_ref(&app))
        .expect("graph context");

    let err = apply_start_satisfaction(
        &mut services,
        &mut operations,
        &mut graph,
        satisfaction_request("app", operation_id),
    )
    .expect_err("not running");

    assert_eq!(
        err,
        StartSatisfactionError::OperationNotRunning {
            operation_id,
            state: OperationState::Pending,
        }
    );
}
