use crate::execution::start::{
    RestartStartExecutionOutcome, StartExecutionStore, StartPreCheckTerminalOutcome,
    begin_restart_start_leg,
};
use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::OperationState;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::super::{operation_id, service};
use super::support::{registry_check, restart_request, running_restart_operation};

#[test]
fn restart_start_leg_condition_skip_completes_restart_without_job() {
    let mut definition = service("app");
    definition.conditions = vec![registry_check("Machine\\System\\Services\\missing")];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = running_restart_operation("app");
    let operation_id = operation_id(0);
    let mut graph = crate::execution::graph::GraphExecutionStore::new();
    let mut jobs = JobStore::new();
    let mut job_ids = JobIdAllocator::new();
    let mut start_store = StartExecutionStore::new();

    let outcome = begin_restart_start_leg(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        restart_request("app", operation_id),
    )
    .expect("restart condition skip");

    let RestartStartExecutionOutcome::Terminal(dispatch) = outcome else {
        panic!("expected terminal restart condition outcome");
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
        operations.get(operation_id).expect("operation").state,
        OperationState::Completed
    );
    assert_eq!(jobs.active_for_service("app"), Vec::new());
    assert_eq!(job_ids.next_sequence(), 0);
    assert_eq!(dispatch.service_transitions.len(), 2);
}

#[test]
fn restart_start_leg_assert_failure_fails_restart_without_job() {
    let mut definition = service("app");
    definition.asserts = vec![registry_check("Machine\\System\\Services\\missing")];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = running_restart_operation("app");
    let operation_id = operation_id(0);
    let mut graph = crate::execution::graph::GraphExecutionStore::new();
    let mut jobs = JobStore::new();
    let mut job_ids = JobIdAllocator::new();
    let mut start_store = StartExecutionStore::new();

    let outcome = begin_restart_start_leg(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        restart_request("app", operation_id),
    )
    .expect("restart assert failure");

    let RestartStartExecutionOutcome::Terminal(dispatch) = outcome else {
        panic!("expected terminal restart assert outcome");
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
    assert_eq!(dispatch.service_transitions.len(), 2);
}

// PEI-370. §6.2's failure propagation is unconditional, and an assert failing
// takes the service to Failed with cause AssertionError — so the propagation
// applies. Every assert path went through apply_start_failure, which runs
// graph.apply_operation_failed, except the restart leg: it transitioned and
// failed the operation itself, so no graph event was ever dispatched.
//
// That is the wrong path to skip. A dependent is allowed to keep running when
// its dependency *crashes* (§6.1) because a crash is transient and the restart
// policy handles it. An AssertionError is never restarted — so the one assert
// path that skipped propagation was the one where the dependency is most
// permanently gone.
#[test]
fn a_restart_legs_assert_failure_propagates_through_the_graph() {
    let mut definition = service("app");
    definition.asserts = vec![registry_check("Machine\\System\\Services\\missing")];
    let mut services = ServiceTable::from_boot_snapshot(vec![definition]).expect("service table");
    let mut operations = running_restart_operation("app");
    let operation_id = operation_id(0);
    let mut graph = crate::execution::graph::GraphExecutionStore::new();
    graph
        .create_on_demand_context(
            &super::support::on_demand_dispatch(
                "app",
                crate::operation::store::OperationRequestOutcome {
                    returned_operation_id: operation_id,
                    stored_operation_id: operation_id,
                    decision: crate::operation::conflict::OperationConflictDecision::CreateNew,
                    events: Vec::new(),
                },
            ),
            &services,
        )
        .expect("graph context");
    let mut jobs = JobStore::new();
    let mut job_ids = JobIdAllocator::new();
    let mut start_store = StartExecutionStore::new();

    let outcome = begin_restart_start_leg(
        &mut services,
        &mut operations,
        &mut graph,
        &mut jobs,
        &mut job_ids,
        &mut start_store,
        restart_request("app", operation_id),
    )
    .expect("assert failure");

    let RestartStartExecutionOutcome::Terminal(dispatch) = outcome else {
        panic!("expected terminal assert outcome");
    };
    assert_eq!(
        dispatch.outcome,
        StartPreCheckTerminalOutcome::AssertionFailed {
            check: "registry:Machine\\System\\Services\\missing".to_string(),
        },
    );
    // The outcome the ticket is about: a graph event, so dependents in the
    // same context hear that the dependency failed.
    assert_eq!(
        dispatch.graph_events[0].outcome,
        crate::execution::graph::GraphTerminalOutcome::Failed,
    );
    // And the service and operation land exactly where the hand-rolled path
    // used to put them, which is what makes this a routing change and not a
    // behaviour change for the service itself.
    assert_eq!(
        services.runtime("app").expect("runtime").state,
        ServiceState::Failed,
    );
    assert_eq!(
        services.runtime("app").expect("runtime").cause,
        Some(TransitionCause::AssertionError),
    );
    assert_eq!(
        operations.get(operation_id).expect("operation").state,
        OperationState::Failed,
    );
    assert!(jobs.active_for_service("app").is_empty());
}
