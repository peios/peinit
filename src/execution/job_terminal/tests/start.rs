use crate::execution::job_terminal::apply_service_main_job_terminal;
use crate::execution::test_support::{StartedBootGraph, service};
use crate::job::JobEventDetail;
use crate::operation::OperationState;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::service::{Readiness, ServiceType};

use super::success::ENDED_AT_NS;

#[test]
fn non_retained_oneshot_successful_exit_satisfies_start_operation_and_clears() {
    let worker = service("worker", ServiceType::Oneshot);
    let mut fixture = StartedBootGraph::new(vec![worker], "worker");
    fixture.mark_started_job_running();
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 0)
        .expect("complete job");

    let dispatch = apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("apply terminal job");

    assert_eq!(
        fixture.services.runtime("worker").expect("runtime").state,
        ServiceState::Inactive
    );
    assert_eq!(
        fixture
            .operations
            .get(fixture.started_operation_id)
            .expect("operation")
            .state,
        OperationState::Completed
    );
    assert_eq!(dispatch.operation_events.len(), 1);
    assert_eq!(
        dispatch
            .service_transitions
            .iter()
            .map(|transition| transition.event.to)
            .collect::<Vec<_>>(),
        vec![ServiceState::Completed, ServiceState::Inactive]
    );
    assert_eq!(dispatch.graph_events.len(), 1);
    assert!(matches!(
        dispatch.job_event.detail,
        JobEventDetail::Ended {
            ended_at_ns: ENDED_AT_NS,
            exit_code: Some(0),
            ..
        }
    ));
}

#[test]
fn oneshot_success_exit_code_satisfies_start_operation() {
    let mut worker = service("worker", ServiceType::Oneshot);
    worker.success_exit_codes = vec![2];
    worker.remain_after_exit = true;
    let mut fixture = StartedBootGraph::new(vec![worker], "worker");
    fixture.mark_started_job_running();
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 2)
        .expect("complete job");

    apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("apply terminal job");

    assert_eq!(
        fixture.services.runtime("worker").expect("runtime").state,
        ServiceState::Completed
    );
}

#[test]
fn starting_simple_process_exit_fails_start_operation() {
    let mut app = service("app", ServiceType::Simple);
    app.readiness = Readiness::Notify;
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 1)
        .expect("complete job");

    let dispatch = apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("apply terminal job");

    let runtime = fixture.services.runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Backoff);
    assert_eq!(runtime.cause, Some(TransitionCause::ProcessCrash));
    assert_eq!(
        fixture
            .operations
            .get(fixture.started_operation_id)
            .expect("operation")
            .state,
        OperationState::Failed
    );
    assert_eq!(dispatch.operation_events.len(), 1);
    assert_eq!(dispatch.service_transitions.len(), 1);
    // Backoff holds the graph member rather than failing it: the service is
    // going to start again, so nothing terminal has happened to its
    // dependents (§6.1, PEI-821).
    assert!(dispatch.graph_events.is_empty());
    assert!(fixture.graph.is_awaiting_restart("app"));
}
