use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::operation::{OperationSource, OperationState};
use crate::service::runtime::ServiceState;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    ADMIN_START_NS, BOOT_NS, DB_LAUNCH_NS, ON_DEMAND_APP_LAUNCH_NS, ScriptedClock, StaticRegistry,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};

#[test]
fn admin_start_launches_dependency_then_requested_service() {
    let mut app = alive_service("app");
    app.triggers.clear();
    app.requires.push("db".to_string());
    let mut db = alive_service("db");
    db.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, db]);
    let mut clock = ScriptedClock::new([
        BOOT_NS,
        ADMIN_START_NS,
        DB_LAUNCH_NS,
        ON_DEMAND_APP_LAUNCH_NS,
    ]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    assert!(boot.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());

    let start = supervisor
        .start_service("app", None, &mut clock)
        .expect("start app");
    let LifecycleCommandOutcome::OnDemandStart(dispatch) = &start.outcome else {
        panic!("expected on-demand start");
    };
    assert!(start.context_id.is_some());
    assert_eq!(
        start
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["db"],
    );
    assert_eq!(
        dispatch
            .events
            .iter()
            .map(|event| event.service.as_str())
            .collect::<Vec<_>>(),
        vec!["db", "app"],
    );
    let requested_operation = dispatch.requested_operation.returned_operation_id;
    let dependency_operation = dispatch.dependency_operations[0].returned_operation_id;

    assert_eq!(
        supervisor
            .operation_status(requested_operation)
            .expect("requested operation")
            .source,
        OperationSource::Admin,
    );
    assert_eq!(
        supervisor
            .operation_status(dependency_operation)
            .expect("dependency operation")
            .source,
        OperationSource::DependencyPropagation,
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(7000, 40), process(7001, 41)]);
    let db_launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch db")
        .expect("db launch dispatch");
    assert_eq!(
        db_launch
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    assert_eq!(
        supervisor.service_status("db").expect("db status").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor
            .operation_status(dependency_operation)
            .expect("completed dependency")
            .state,
        OperationState::Completed,
    );

    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    assert_eq!(tokens.observed_jobs, vec!["db", "app"]);
    assert_eq!(launcher.observed_jobs, vec!["db", "app"]);
    assert_eq!(
        supervisor.service_status("app").expect("app status").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor
            .operation_status(requested_operation)
            .expect("completed requested")
            .state,
        OperationState::Completed,
    );
}

// PEI-364. `GraphExecutionContext::is_drained()` existed and nothing removed a
// context or its operation associations, so every boot and every explicit
// start leaked both for the life of the process.
//
// The memory was the smaller half. `apply_operation_terminal` walks *every*
// context associated with an operation, and associations were never dropped —
// so on a machine up for months, where an operator or a script starts services
// regularly, the terminal path got steadily slower, in PID 1's single thread.
#[test]
fn a_drained_graph_context_is_retired_with_its_associations() {
    let mut app = alive_service("app");
    app.triggers.clear();
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, ADMIN_START_NS, ON_DEMAND_APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
        .start_service("app", None, &mut clock)
        .expect("start app");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9000, 90)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    // The boot context plus the on-demand one.
    assert_eq!(supervisor.graph().context_count(), 2);

    let maintenance = supervisor
        .process_due_operation_maintenance(ON_DEMAND_APP_LAUNCH_NS + 1)
        .expect("maintenance");

    assert_eq!(maintenance.retired_graph_contexts.len(), 2);
    assert_eq!(
        supervisor.graph().context_count(),
        0,
        "a drained context can dispatch no further event and was still held",
    );
    assert_eq!(
        supervisor.graph().association_count(),
        0,
        "apply_operation_terminal still has associations to walk",
    );
}
