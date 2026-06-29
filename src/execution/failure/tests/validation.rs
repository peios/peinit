use crate::execution::failure::{StartFailureError, StartFailureRequest, apply_start_failure};
use crate::execution::test_support::{StartedBootGraph, service};
use crate::operation::OperationState;
use crate::service::ServiceType;
use crate::service::runtime::TransitionCause;

const FAILED_AT_NS: u64 = 1_000_003_000;

#[test]
fn failure_rejects_wrong_service_without_mutation() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();
    let operation_id = fixture.started_operation_id;

    let err = apply_start_failure(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        failure_request("other", operation_id, TransitionCause::ReadinessTimeout),
    )
    .expect_err("service mismatch");

    assert_eq!(
        err,
        StartFailureError::OperationServiceMismatch {
            operation_id,
            expected_service: "other".to_string(),
            actual_service: "app".to_string(),
        }
    );
    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
}

#[test]
fn failure_rolls_back_when_failure_cause_cannot_transition_service() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();

    apply_start_failure(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        failure_request(
            "app",
            fixture.started_operation_id,
            TransitionCause::ExplicitStart,
        ),
    )
    .expect_err("invalid failure cause");

    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
}

#[test]
fn failure_rejects_completed_operation_without_mutation() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let operation_id = fixture.started_operation_id;
    crate::execution::satisfaction::apply_start_satisfaction(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        crate::execution::test_support::satisfaction_request("app", operation_id),
    )
    .expect("satisfy first");
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();

    let err = apply_start_failure(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        failure_request("app", operation_id, TransitionCause::ReadinessTimeout),
    )
    .expect_err("not running");

    assert_eq!(
        err,
        StartFailureError::OperationNotRunning {
            operation_id,
            state: OperationState::Completed,
        }
    );
    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
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
        reason: "failure".to_string(),
    }
}
