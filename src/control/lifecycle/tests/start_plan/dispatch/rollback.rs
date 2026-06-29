use crate::control::lifecycle::{
    OnDemandStartDispatchError, dispatch_on_demand_start_plan, plan_on_demand_start,
};
use crate::ids::OperationIdAllocator;
use crate::operation::store::{OperationRequest, OperationStore};
use crate::operation::{OperationSource, OperationType};

use super::super::{OBSERVED_AT_NS, operation_ids, service, table};

#[test]
fn dispatch_rolls_back_store_when_dependency_id_allocation_fails() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let services = table(vec![app, service("db")]);
    let plan = plan_on_demand_start(&services, "app").expect("start plan");
    let requested_id = operation_ids(1)[0];
    let mut dependency_ids = OperationIdAllocator::with_next_sequence(u64::MAX);
    let mut operations = OperationStore::new();

    let error = dispatch_on_demand_start_plan(
        &mut operations,
        &mut dependency_ids,
        plan,
        requested_id,
        None,
        OBSERVED_AT_NS,
    )
    .expect_err("allocation error");

    assert!(matches!(error, OnDemandStartDispatchError::IdAllocation(_)));
    assert_eq!(dependency_ids.next_sequence(), u64::MAX);
    assert!(operations.active_for_service("app").is_empty());
    assert!(operations.active_for_service("db").is_empty());
    assert!(operations.get(requested_id).is_none());
}

#[test]
fn dispatch_rolls_back_store_and_allocator_when_operation_request_fails() {
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
    let original_dependency_ids = dependency_ids.clone();
    let mut operations = OperationStore::new();
    operations
        .request_operation(OperationRequest {
            id: ids[1],
            operation_type: OperationType::Start,
            service: "other".to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: OBSERVED_AT_NS,
        })
        .expect("preexisting duplicate id");
    let original_operations = operations.clone();

    let error = dispatch_on_demand_start_plan(
        &mut operations,
        &mut dependency_ids,
        plan,
        requested_id,
        None,
        OBSERVED_AT_NS,
    )
    .expect_err("operation store error");

    assert!(matches!(
        error,
        OnDemandStartDispatchError::OperationStore(_)
    ));
    assert_eq!(dependency_ids, original_dependency_ids);
    assert_eq!(operations, original_operations);
}
