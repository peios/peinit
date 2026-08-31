//! A main process exiting after its service stopped expecting one.

use crate::execution::job_terminal::apply_service_main_job_terminal;
use crate::execution::test_support::{StartedBootGraph, service};
use crate::service::ServiceType;
use crate::service::runtime::ServiceState;

use super::success::{ENDED_AT_NS, satisfy_start};

// PEI-531, the state jack saw on the console: `UnsupportedServiceState {
// service: "atriumd", state: Backoff }`, which the runtime loop reports as a
// loop failure and PID 1 answers by dropping into Recovery — killing every
// session on the machine because one service was crash-looping.
//
// `Backoff` is an ordinary state, not a broken invariant: it is what
// `RestartPolicy` produces after repeated exits. A service can reach it by a
// route other than its own main exit — a health-check escalation, a watchdog
// timeout — and its process can outlive that transition. The exit that follows
// must cost that one service, and cost it nothing.
#[test]
fn a_late_exit_in_backoff_is_recorded_rather_than_failing_the_runtime_loop() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    satisfy_start(&mut fixture);
    let job_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 1)
        .expect("complete job");
    apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event.clone())
        .expect("the crash that puts app into backoff");
    assert_eq!(
        fixture.services.runtime("app").expect("runtime").state,
        ServiceState::Backoff,
    );
    let before_services = fixture.services.clone();
    let before_operations = fixture.operations.clone();
    let before_graph = fixture.graph.clone();

    let dispatch = apply_service_main_job_terminal(&mut fixture.start_ready_context(), job_event)
        .expect("a late exit is not a runtime failure");

    assert_eq!(dispatch.late_exit, Some(ServiceState::Backoff));
    assert!(dispatch.service_transitions.is_empty());
    assert!(dispatch.operation_events.is_empty());
    assert!(dispatch.graph_events.is_empty());
    assert!(dispatch.post_start_hook.is_none());
    // Recorded, not acted on. The backoff deadline that was already set is what
    // restarts the service; a late exit must not disturb it, or a crash-looping
    // service would have its restart pushed out by its own stale exits.
    assert_eq!(fixture.services, before_services);
    assert_eq!(fixture.operations, before_operations);
    assert_eq!(fixture.graph, before_graph);
}
