use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::execution::graph::GraphTerminalOutcome;
use crate::execution::test_support::{StartedBootGraph, service};
use crate::operation::OperationState;
use crate::service::ServiceType;
use crate::service::runtime::{ServiceState, TransitionCause};

const FAILED_AT_NS: u64 = 1_000_003_000;

#[test]
fn start_failure_marks_operation_failed_service_backoff_and_hard_dependents_failed() {
    let registry = service("registry", ServiceType::Simple);
    let mut app = service("app", ServiceType::Simple);
    app.requires.push("registry".to_string());
    let mut fixture = StartedBootGraph::new(vec![registry, app], "registry");
    let operation_id = fixture.started_operation_id;

    let dispatch = apply_start_failure(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        failure_request("registry", operation_id, TransitionCause::ReadinessTimeout),
    )
    .expect("fail start");

    assert_runtime(
        &fixture,
        "registry",
        ServiceState::Backoff,
        TransitionCause::ReadinessTimeout,
    );
    assert_eq!(
        fixture
            .services
            .runtime("registry")
            .expect("registry runtime")
            .restart_backoff_until_ns,
        Some(FAILED_AT_NS + 1_000_000_000),
    );
    assert_runtime(
        &fixture,
        "app",
        ServiceState::Failed,
        TransitionCause::DependencyFailure,
    );
    assert_eq!(
        dispatch
            .graph_events
            .iter()
            .map(|event| (event.service.as_str(), event.outcome))
            .collect::<Vec<_>>(),
        vec![
            ("registry", GraphTerminalOutcome::Failed),
            ("app", GraphTerminalOutcome::Failed)
        ]
    );
    assert_eq!(dispatch.operation_events.len(), 2);
    for event in &dispatch.graph_events {
        assert_eq!(
            fixture
                .operations
                .get(event.operation_id)
                .expect("operation")
                .state,
            OperationState::Failed
        );
    }
}

fn failure_request(
    service: &str,
    operation_id: crate::ids::OperationId,
    failure_cause: TransitionCause,
) -> StartFailureRequest {
    StartFailureRequest {
        service: service.to_string(),
        operation_id,
        failed_at_ns: FAILED_AT_NS,
        failure_cause,
        reason: "readiness timeout".to_string(),
        exit_code: None,
    }
}

fn assert_runtime(
    fixture: &StartedBootGraph,
    service: &str,
    state: ServiceState,
    cause: TransitionCause,
) {
    let runtime = fixture.services.runtime(service).expect("runtime");
    assert_eq!(runtime.state, state);
    assert_eq!(runtime.cause, Some(cause));
}
