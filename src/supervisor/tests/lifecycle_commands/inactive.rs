use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::service::runtime::ServiceState;

use super::booted_supervisor;
use crate::supervisor::tests::{LIFECYCLE_COMMAND_NS, ScriptedClock, alive_service};

#[test]
fn stop_on_inactive_service_is_noop_without_storing_operation() {
    let mut app = alive_service("app");
    app.triggers.clear();
    let mut supervisor = booted_supervisor(vec![app]);
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);

    let dispatch = supervisor
        .stop_service("app", None, &mut clock)
        .expect("stop inactive app");

    assert!(matches!(dispatch.outcome, LifecycleCommandOutcome::Noop(_)));
    assert!(dispatch.context_id.is_none());
    assert!(dispatch.start_dispatches.is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("app status").state,
        ServiceState::Inactive,
    );
}
