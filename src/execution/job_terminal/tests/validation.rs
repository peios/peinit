use crate::execution::job_terminal::{
    ServiceMainJobTerminalError, apply_service_main_job_terminal,
};
use crate::execution::test_support::{StartedBootGraph, service};
use crate::job::JobEventDetail;
use crate::service::{ServiceTableError, ServiceType};

#[test]
fn non_terminal_job_event_is_rejected_without_mutation() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();
    let event = fixture
        .jobs
        .get(fixture.started_job_id)
        .map(crate::job::JobEvent::created)
        .expect("created event");

    let err = apply_service_main_job_terminal(&mut fixture.start_ready_context(), event)
        .expect_err("not terminal");

    assert_eq!(
        err,
        ServiceMainJobTerminalError::NotTerminalEvent {
            job_id: fixture.started_job_id,
        }
    );
    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
}

#[test]
fn unknown_service_terminal_event_is_reported_without_mutation() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    let mut event = fixture
        .jobs
        .complete_job(fixture.started_job_id, 1_000_004_000, 0)
        .expect("complete job");
    event.service = Some("missing".to_string());
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();

    let err = apply_service_main_job_terminal(&mut fixture.start_ready_context(), event)
        .expect_err("unknown service");

    assert_eq!(
        err,
        ServiceMainJobTerminalError::ServiceTable(ServiceTableError::UnknownService {
            service: "missing".to_string(),
        })
    );
    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
}

#[test]
fn starting_service_terminal_event_requires_operation_id() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    let mut event = fixture
        .jobs
        .complete_job(fixture.started_job_id, 1_000_004_000, 1)
        .expect("complete job");
    event.operation_id = None;

    let err = apply_service_main_job_terminal(&mut fixture.start_ready_context(), event)
        .expect_err("missing operation");

    assert_eq!(
        err,
        ServiceMainJobTerminalError::MissingOperation {
            job_id: fixture.started_job_id,
            service: "app".to_string(),
        }
    );
}

#[test]
fn terminal_detail_includes_end_timestamp() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();

    let event = fixture
        .jobs
        .complete_job(fixture.started_job_id, 1_000_004_000, 0)
        .expect("complete job");

    assert!(matches!(
        event.detail,
        JobEventDetail::Ended {
            ended_at_ns: 1_000_004_000,
            ..
        }
    ));
}
