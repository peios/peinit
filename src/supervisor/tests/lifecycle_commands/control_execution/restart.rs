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

// PEI-345. §8.1: "If the target service becomes definition-removed while a
// Restart operation is Running in its stop phase, the stop phase still drains
// the existing instance, but peinit MUST NOT begin the start phase. When the
// stopped instance exits, peinit MUST abort the Restart operation with reason
// `definition_removed_during_restart_stop_leg`."
//
// The reason string did not exist anywhere in the tree, and neither did an
// abort path: OperationStore had start/complete/fail/cancel and nothing else,
// so `Aborted` was reachable only through conflict supersession. What happened
// instead was that the start leg looked up a definition the stop-leg
// transition had just discarded, found None, and raised
// MissingStartCredentials — an internal error out of the terminal-job path,
// which is not a control-flow outcome, so the rest of that turn's work did not
// happen either.
//
// The window is exactly the one a package upgrade opens: remove the
// definition, then restart the service.
#[test]
fn a_definition_withdrawn_during_a_restarts_stop_leg_aborts_the_operation() {
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
    let old_job = current_app_job(&supervisor);

    // The registry stops defining it while the stop leg is draining. Stopping
    // retains the entry, so the instance is still supervised to the end.
    supervisor
        .services
        .apply_definition_snapshot(Vec::new())
        .expect("withdraw the definition");

    let terminal = supervisor
        .complete_job(old_job, EXIT_NS, 0)
        .expect("the stop leg drains without an internal error");

    // The stop drained. No start leg.
    assert!(terminal.restart_start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    // Aborted, not failed: nothing about the operation went wrong, its subject
    // stopped existing.
    let status = supervisor
        .operation_status(operation_id)
        .expect("restart operation");
    assert_eq!(status.state, OperationState::Aborted);
    // The reason surfaces as `error`, which is where a non-Completed
    // operation's result is projected.
    assert_eq!(
        status.error.as_deref(),
        Some("definition_removed_during_restart_stop_leg"),
    );
    // And the entry is gone, as the normal service-removal discard requires.
    assert!(supervisor.service_status("app").is_err());
}
