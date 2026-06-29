use crate::control::lifecycle::{dispatch_on_demand_start_plan, plan_on_demand_start};
use crate::ids::OperationIdAllocator;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::{OperationEventDetail, OperationStore};

use super::super::{OBSERVED_AT_NS, operation_ids, service, table};

#[test]
fn dispatch_requests_dependency_operations_before_requested_operation() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let services = table(vec![app, service("db")]);
    let plan = plan_on_demand_start(&services, "app").expect("start plan");
    let ids = operation_ids(2);
    let requested_id = ids[0];
    let mut dependency_ids = OperationIdAllocator::new();
    dependency_ids
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("skip requested id");
    let mut operations = OperationStore::new();

    let dispatch = dispatch_on_demand_start_plan(
        &mut operations,
        &mut dependency_ids,
        plan,
        requested_id,
        None,
        OBSERVED_AT_NS,
    )
    .expect("dispatch");

    assert_eq!(dispatch.dependency_operations.len(), 1);
    assert_eq!(
        dispatch.dependency_operations[0].decision,
        OperationConflictDecision::CreateNew
    );
    assert_eq!(
        dispatch.dependency_operations[0].returned_operation_id,
        ids[1]
    );
    assert_eq!(
        dispatch.requested_operation.returned_operation_id,
        requested_id
    );
    assert_eq!(
        dispatch
            .events
            .iter()
            .map(|event| (event.service.as_str(), event.detail.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("db", OperationEventDetail::Requested),
            ("app", OperationEventDetail::Requested),
        ]
    );
    assert_eq!(operations.active_for_service("db"), vec![ids[1]]);
    assert_eq!(operations.active_for_service("app"), vec![requested_id]);
    assert_eq!(dependency_ids.next_sequence(), 2);
}
