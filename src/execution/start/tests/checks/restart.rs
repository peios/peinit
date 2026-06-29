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
