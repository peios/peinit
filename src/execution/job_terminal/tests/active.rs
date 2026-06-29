use crate::execution::job_terminal::apply_service_main_job_terminal;
use crate::execution::test_support::{StartedBootGraph, service};
use crate::operation::OperationState;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{RestartPolicy, ServiceType};

use super::success::{ENDED_AT_NS, satisfy_start};

#[test]
fn active_simple_clean_exit_transitions_to_inactive_without_touching_operation() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    satisfy_start(&mut fixture);
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 0)
        .expect("complete job");

    let dispatch = apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("apply terminal job");

    let runtime = fixture.services.runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Inactive);
    assert_eq!(runtime.cause, Some(TransitionCause::CleanExit));
    assert_eq!(
        fixture
            .operations
            .get(fixture.started_operation_id)
            .expect("operation")
            .state,
        OperationState::Completed
    );
    assert!(dispatch.operation_events.is_empty());
    assert_eq!(dispatch.service_transitions.len(), 1);
    assert!(dispatch.graph_events.is_empty());
}

#[test]
fn active_simple_crash_enters_restart_backoff_when_policy_allows() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    satisfy_start(&mut fixture);
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 1)
        .expect("complete job");

    apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("apply terminal job");

    let runtime = fixture.services.runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Backoff);
    assert_eq!(runtime.cause, Some(TransitionCause::ProcessCrash));
    assert_eq!(runtime.consecutive_restart_failures, 1);
    assert_eq!(
        runtime.restart_backoff_until_ns,
        Some(ENDED_AT_NS + 1_000_000_000)
    );
}

#[test]
fn active_simple_clean_exit_with_always_policy_enters_backoff() {
    let mut app = service("app", ServiceType::Simple);
    app.restart_policy = RestartPolicy::Always;
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    satisfy_start(&mut fixture);
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 0)
        .expect("complete job");

    apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("apply terminal job");

    let runtime = fixture.services.runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Backoff);
    assert_eq!(runtime.cause, Some(TransitionCause::CleanExitRestart));
}

#[test]
fn restart_budget_exhaustion_transitions_to_failed() {
    let mut app = service("app", ServiceType::Simple);
    app.restart_max_retries = 0;
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    satisfy_start(&mut fixture);
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 1)
        .expect("complete job");

    apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("apply terminal job");

    let runtime = fixture.services.runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Failed);
    assert_eq!(runtime.cause, Some(TransitionCause::RestartBudgetExhausted));
}

#[test]
fn reloading_crash_with_exhausted_budget_transitions_to_failed() {
    let mut app = service("app", ServiceType::Simple);
    app.restart_max_retries = 0;
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    satisfy_start(&mut fixture);
    fixture
        .services
        .transition_service(
            "app",
            ServiceTransition {
                to: ServiceState::Reloading,
                cause: TransitionCause::ExplicitReload,
            },
        )
        .expect("reloading");
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 1)
        .expect("complete job");

    apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("apply terminal job");

    let runtime = fixture.services.runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Failed);
    assert_eq!(runtime.cause, Some(TransitionCause::RestartBudgetExhausted));
}
