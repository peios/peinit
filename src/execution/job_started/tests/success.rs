use crate::execution::graph::GraphTerminalOutcome;
use crate::execution::job_started::apply_service_main_job_started;
use crate::execution::test_support::{StartedBootGraph, service};
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;
use crate::service::{Readiness, ServiceType};

#[test]
fn alive_readiness_satisfies_start_when_service_main_job_starts() {
    let mut app = service("app", ServiceType::Simple);
    app.readiness = Readiness::Alive;
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let event = fixture.mark_started_job_running();

    let dispatch = apply_service_main_job_started(&mut fixture.start_ready_context(), event)
        .expect("apply started job");

    assert_eq!(
        fixture.services.runtime("app").expect("runtime").state,
        ServiceState::Active
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
    assert_eq!(dispatch.service_transitions.len(), 1);
    assert_eq!(
        dispatch
            .graph_events
            .iter()
            .map(|event| (event.service.as_str(), event.outcome))
            .collect::<Vec<_>>(),
        vec![("app", GraphTerminalOutcome::Satisfied)]
    );
}

#[test]
fn notify_readiness_waits_for_a_later_readiness_event() {
    let mut app = service("app", ServiceType::Simple);
    app.readiness = Readiness::Notify;
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let event = fixture.mark_started_job_running();

    let dispatch = apply_service_main_job_started(&mut fixture.start_ready_context(), event)
        .expect("apply started job");

    assert_eq!(
        fixture.services.runtime("app").expect("runtime").state,
        ServiceState::Starting
    );
    assert_eq!(
        fixture
            .operations
            .get(fixture.started_operation_id)
            .expect("operation")
            .state,
        OperationState::Running
    );
    assert!(dispatch.operation_events.is_empty());
    assert!(dispatch.service_transitions.is_empty());
    assert!(dispatch.graph_events.is_empty());
}

#[test]
fn oneshot_started_job_waits_for_terminal_exit() {
    let worker = service("worker", ServiceType::Oneshot);
    let mut fixture = StartedBootGraph::new(vec![worker], "worker");
    let event = fixture.mark_started_job_running();

    let dispatch = apply_service_main_job_started(&mut fixture.start_ready_context(), event)
        .expect("apply started job");

    assert_eq!(
        fixture.services.runtime("worker").expect("runtime").state,
        ServiceState::Starting
    );
    assert!(dispatch.operation_events.is_empty());
    assert!(dispatch.service_transitions.is_empty());
    assert!(dispatch.graph_events.is_empty());
}
