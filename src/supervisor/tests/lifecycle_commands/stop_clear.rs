use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;

use super::completed_oneshot_supervisor;
use crate::supervisor::tests::{LIFECYCLE_COMMAND_NS, ScriptedClock};

#[test]
fn stop_on_completed_service_clears_state_and_completes_operation() {
    let mut supervisor = completed_oneshot_supervisor();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);

    let dispatch = supervisor
        .stop_service("task", None, &mut clock)
        .expect("stop completed task");
    let LifecycleCommandOutcome::SynchronousClear(clear) = dispatch.outcome else {
        panic!("expected synchronous clear");
    };

    let operation_id = clear.request.returned_operation_id;
    assert!(dispatch.context_id.is_none());
    assert!(dispatch.start_dispatches.is_empty());
    assert_eq!(clear.service_transition.event.to, ServiceState::Inactive);
    assert_eq!(
        supervisor
            .service_status("task")
            .expect("task status")
            .state,
        ServiceState::Inactive,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("clear operation")
            .state,
        OperationState::Completed,
    );
}
