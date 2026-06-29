use crate::execution::graph::GraphTerminalOutcome;
use crate::execution::satisfaction::apply_start_satisfaction;
use crate::operation::OperationState;
use crate::service::ServiceType;
use crate::service::runtime::ServiceState;

use crate::execution::test_support::{StartedBootGraph, satisfaction_request, service};

#[test]
fn simple_service_satisfaction_completes_operation_and_releases_dependents() {
    let registry = service("registry", ServiceType::Simple);
    let mut app = service("app", ServiceType::Simple);
    app.requires.push("registry".to_string());
    let mut fixture = StartedBootGraph::new(vec![registry, app], "registry");
    let operation_id = fixture.started_operation_id;

    let dispatch = apply_start_satisfaction(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        satisfaction_request("registry", operation_id),
    )
    .expect("satisfy start");

    assert_eq!(
        fixture.services.runtime("registry").expect("runtime").state,
        ServiceState::Active
    );
    assert_eq!(
        fixture
            .operations
            .get(operation_id)
            .expect("operation")
            .state,
        OperationState::Completed
    );
    assert_eq!(
        dispatch
            .graph_events
            .iter()
            .map(|event| (event.service.as_str(), event.outcome))
            .collect::<Vec<_>>(),
        vec![("registry", GraphTerminalOutcome::Satisfied)]
    );

    let ready = fixture
        .graph
        .release_ready(fixture.context_id, 10)
        .expect("release dependent");
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].service, "app");
}

#[test]
fn non_retained_oneshot_satisfaction_releases_dependents_then_clears_to_inactive() {
    let worker = service("worker", ServiceType::Oneshot);
    let mut fixture = StartedBootGraph::new(vec![worker], "worker");
    let operation_id = fixture.started_operation_id;

    let dispatch = apply_start_satisfaction(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        satisfaction_request("worker", operation_id),
    )
    .expect("satisfy oneshot");

    assert_eq!(
        fixture.services.runtime("worker").expect("runtime").state,
        ServiceState::Inactive
    );
    assert_eq!(
        dispatch
            .service_transitions
            .iter()
            .map(|transition| transition.event.to)
            .collect::<Vec<_>>(),
        vec![ServiceState::Completed, ServiceState::Inactive]
    );
}

#[test]
fn retained_oneshot_satisfaction_stays_completed() {
    let mut worker = service("worker", ServiceType::Oneshot);
    worker.remain_after_exit = true;
    let mut fixture = StartedBootGraph::new(vec![worker], "worker");
    let operation_id = fixture.started_operation_id;

    let dispatch = apply_start_satisfaction(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        satisfaction_request("worker", operation_id),
    )
    .expect("satisfy oneshot");

    assert_eq!(
        fixture.services.runtime("worker").expect("runtime").state,
        ServiceState::Completed
    );
    assert_eq!(dispatch.service_transitions.len(), 1);
}
