use crate::control::lifecycle::{
    dispatch_on_demand_start_plan, plan_on_demand_start, plan_restart_policy_start,
};
use crate::ids::OperationIdAllocator;
use crate::operation::store::{OperationEventDetail, OperationStore};
use crate::operation::{OperationSource, OperationState};

use super::super::{OBSERVED_AT_NS, operation_ids, service, table};

#[test]
fn dispatch_fails_requested_operation_when_plan_is_blocked() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let services = table(vec![app]);
    let plan = plan_on_demand_start(&services, "app").expect("blocked plan");
    let requested_id = operation_ids(1)[0];
    let mut dependency_ids = OperationIdAllocator::new();
    let mut operations = OperationStore::new();

    let dispatch = dispatch_on_demand_start_plan(
        &mut operations,
        &mut dependency_ids,
        plan,
        requested_id,
        None,
        OBSERVED_AT_NS,
    )
    .expect("blocked dispatch");

    assert!(dispatch.dependency_operations.is_empty());
    assert_eq!(
        dispatch
            .events
            .iter()
            .map(|event| event.detail.clone())
            .collect::<Vec<_>>(),
        vec![
            OperationEventDetail::Requested,
            OperationEventDetail::Failed {
                duration_ns: 0,
                failure_reason: "DependencyFailure: Requires dependency db is unavailable"
                    .to_string(),
            },
        ]
    );
    assert_eq!(
        operations
            .get(requested_id)
            .expect("requested operation")
            .state,
        OperationState::Failed,
    );
    assert!(operations.active_for_service("app").is_empty());
    assert_eq!(dependency_ids.next_sequence(), 0);
}

#[test]
fn blocked_restart_policy_dispatch_uses_restart_policy_source() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let services = table(vec![app]);
    let plan = plan_restart_policy_start(&services, "app").expect("blocked restart plan");
    let requested_id = operation_ids(1)[0];
    let mut dependency_ids = OperationIdAllocator::new();
    let mut operations = OperationStore::new();

    dispatch_on_demand_start_plan(
        &mut operations,
        &mut dependency_ids,
        plan,
        requested_id,
        None,
        OBSERVED_AT_NS,
    )
    .expect("blocked dispatch");

    assert_eq!(
        operations
            .get(requested_id)
            .expect("requested operation")
            .source,
        OperationSource::RestartPolicy,
    );
}
