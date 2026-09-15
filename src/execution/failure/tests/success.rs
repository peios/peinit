use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::execution::graph::{GraphMemberStatus, GraphTerminalOutcome};
use crate::execution::test_support::{StartedBootGraph, service};
use crate::operation::OperationState;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::service::{RestartPolicy, ServiceType};

const FAILED_AT_NS: u64 = 1_000_003_000;

/// The PEI-821 bug: the graph member was failed before the restart policy
/// was consulted, so a hard dependent got DependencyFailure on the way to a
/// Backoff the target then came back from. A service in Backoff is going to
/// start again, and its dependents wait rather than failing (§6.1).
#[test]
fn start_failure_into_backoff_holds_hard_dependents_rather_than_failing_them() {
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
    assert_eq!(
        fixture
            .operations
            .get(operation_id)
            .expect("registry operation")
            .state,
        OperationState::Failed
    );
    // The dependent is held exactly as a start held on a readiness level
    // is: Inactive, with its start operation still Pending — no new state.
    let app = fixture.services.runtime("app").expect("app runtime");
    assert_eq!(app.state, ServiceState::Inactive);
    assert_eq!(app.cause, None);
    let app_operation = fixture
        .operations
        .current_for_service("app")
        .expect("app keeps its start operation");
    assert_eq!(app_operation.state, OperationState::Pending);
    assert!(fixture.graph.is_operation_held(app_operation.id));
    assert!(
        dispatch.graph_events.is_empty(),
        "a hold is not a terminal: nothing to release or propagate yet"
    );
    assert_eq!(dispatch.operation_events.len(), 1);
    assert_eq!(dispatch.service_transitions.len(), 1);
    assert_eq!(
        fixture
            .graph
            .context(fixture.context_id)
            .expect("boot context")
            .members["registry"]
            .status,
        GraphMemberStatus::AwaitingRestart
    );
}

/// A failure the policy does not restart from is decided at once: the
/// target is Failed and its hard dependents fail with it, as before.
#[test]
fn start_failure_the_policy_does_not_restart_fails_hard_dependents_at_once() {
    let mut registry = service("registry", ServiceType::Simple);
    registry.restart_policy = RestartPolicy::Never;
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
        ServiceState::Failed,
        TransitionCause::ReadinessTimeout,
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

/// An exhausted restart budget is the give-up: the target goes Failed with
/// `RestartBudgetExhausted`, and only now do the dependents held for it
/// fail with `DependencyFailure`.
#[test]
fn an_exhausted_restart_budget_fails_the_dependents_held_for_the_restart() {
    let mut registry = service("registry", ServiceType::Simple);
    registry.restart_max_retries = 0;
    let mut app = service("app", ServiceType::Simple);
    app.requires.push("registry".to_string());
    let mut fixture = StartedBootGraph::new(vec![registry, app], "registry");
    let operation_id = fixture.started_operation_id;

    let dispatch = apply_start_failure(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        failure_request("registry", operation_id, TransitionCause::ProcessCrash),
    )
    .expect("fail start");

    assert_runtime(
        &fixture,
        "registry",
        ServiceState::Failed,
        TransitionCause::RestartBudgetExhausted,
    );
    assert_runtime(
        &fixture,
        "app",
        ServiceState::Failed,
        TransitionCause::DependencyFailure,
    );
    assert_eq!(dispatch.operation_events.len(), 2);
    assert_eq!(
        fixture
            .operations
            .current_for_service("app")
            .map(|operation| operation.state),
        None,
        "the dependent's start operation is terminal"
    );
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
