use crate::control::lifecycle::{LifecycleCommand, LifecycleCommandOutcome};
use crate::service::runtime::ServiceState;

use super::active_app_supervisor;
use crate::supervisor::tests::{LIFECYCLE_COMMAND_NS, ScriptedClock};

#[test]
fn generic_lifecycle_start_reports_already_for_active_service() {
    let mut supervisor = active_app_supervisor();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);

    let dispatch = supervisor
        .run_lifecycle_command(LifecycleCommand::Start, "app", None, &mut clock)
        .expect("start active app");

    assert!(matches!(
        dispatch.outcome,
        LifecycleCommandOutcome::Already(_)
    ));
    assert!(dispatch.context_id.is_none());
    assert!(dispatch.start_dispatches.is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("app status").state,
        ServiceState::Active,
    );
}
