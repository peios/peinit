use crate::execution::job_started::{ServiceMainJobStartedError, apply_service_main_job_started};
use crate::execution::test_support::{StartedBootGraph, service};
use crate::job::JobEvent;
use crate::service::runtime::ServiceState;
use crate::service::{Readiness, ServiceTableError, ServiceType};

#[test]
fn non_started_event_is_rejected_without_mutation() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let event = fixture
        .jobs
        .get(fixture.started_job_id)
        .map(JobEvent::created)
        .expect("created event");
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();

    let err = apply_service_main_job_started(&mut fixture.start_ready_context(), event)
        .expect_err("not started");

    assert_eq!(
        err,
        ServiceMainJobStartedError::NotStartedEvent {
            job_id: fixture.started_job_id,
        }
    );
    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
}

#[test]
fn unknown_service_started_event_is_reported_without_mutation() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let mut event = fixture.mark_started_job_running();
    event.service = Some("missing".to_string());
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();

    let err = apply_service_main_job_started(&mut fixture.start_ready_context(), event)
        .expect_err("unknown service");

    assert_eq!(
        err,
        ServiceMainJobStartedError::ServiceTable(ServiceTableError::UnknownService {
            service: "missing".to_string(),
        })
    );
    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
}

#[test]
fn alive_readiness_started_event_requires_operation_id() {
    let mut app = service("app", ServiceType::Simple);
    app.readiness = Readiness::Alive;
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let mut event = fixture.mark_started_job_running();
    event.operation_id = None;

    let err = apply_service_main_job_started(&mut fixture.start_ready_context(), event)
        .expect_err("missing operation");

    assert_eq!(
        err,
        ServiceMainJobStartedError::MissingOperation {
            job_id: fixture.started_job_id,
            service: "app".to_string(),
        }
    );
}

#[test]
fn started_event_after_service_left_starting_is_rejected() {
    let mut app = service("app", ServiceType::Simple);
    app.readiness = Readiness::Alive;
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let event = fixture.mark_started_job_running();
    fixture
        .services
        .transition_service(
            "app",
            crate::service::runtime::ServiceTransition {
                to: ServiceState::Failed,
                cause: crate::service::runtime::TransitionCause::ReadinessTimeout,
            },
        )
        .expect("transition to failed");

    let err = apply_service_main_job_started(&mut fixture.start_ready_context(), event)
        .expect_err("wrong state");

    assert_eq!(
        err,
        ServiceMainJobStartedError::UnsupportedServiceState {
            service: "app".to_string(),
            state: ServiceState::Failed,
        }
    );
}
