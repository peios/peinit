use crate::boundary::ProcessSignal;
use crate::operation::{OperationState, OperationType};
use crate::service::runtime::ServiceState;
use crate::supervisor::tests::TestProcessController;

use super::*;

#[test]
fn restart_execution_stops_then_queues_new_main_job_under_same_operation() {
    let mut supervisor = active_app_supervisor();
    let mut command_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .restart_service("app", None, &mut command_clock)
        .expect("restart app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = accepted.outcome else {
        panic!("expected restart operation");
    };
    let operation_id = operation.returned_operation_id;
    let mut controller = TestProcessController::default();
    let mut control_clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut control_clock)
        .expect("execute restart stop leg")
        .expect("restart dispatch");

    assert_eq!(controller.signals[0].signal, ProcessSignal::Sigterm);
    let old_job = current_app_job(&supervisor);
    let terminal = supervisor
        .complete_job(old_job, EXIT_NS, 0)
        .expect("complete restart stop leg");

    assert_eq!(terminal.restart_start_dispatches.len(), 1);
    assert_eq!(
        terminal.restart_start_dispatches[0].operation_id,
        operation_id
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("restart operation")
            .state,
        OperationState::Running,
    );
    assert_eq!(
        supervisor.pending_launch_jobs(),
        vec![terminal.restart_start_dispatches[0].job_id],
    );

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9000, 60)]);
    let mut launch_clock = ScriptedClock::new([RESTART_LAUNCH_NS]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut launch_clock)
        .expect("launch restarted app")
        .expect("restart launch");

    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("restart operation")
            .operation_type,
        OperationType::Restart,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("restart operation")
            .state,
        OperationState::Completed,
    );
}
