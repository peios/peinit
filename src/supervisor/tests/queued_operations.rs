//! §8.3 conflict rows that used to end the runtime loop (PEI-824): an
//! operation the conflict table queues waits for its predecessor and is then
//! admitted against the service's current state; a stop that aborts a running
//! restart adopts the stop leg in flight; a stop that aborts a running reload
//! fails the reload command's job.

use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::execution::control::{ControlExecutionDetail, ControlOperationKind};
use crate::operation::OperationState;
use crate::operation::conflict::OperationConflictDecision;
use crate::service::runtime::ServiceState;
use crate::supervisor::{PromotedOperationOutcome, Supervisor, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, alive_service, process,
    settings,
};

fn booted_active_app(
    app: crate::service::ServiceDefinition,
) -> (Supervisor, TestTokenProvider, TestProcessLauncher) {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![
        process(5000, 20),
        process(5001, 21),
        process(5002, 22),
    ]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active
    );
    (supervisor, tokens, launcher)
}

fn accepted(
    dispatch: crate::supervisor::SupervisorLifecycleDispatch,
) -> (crate::ids::OperationId, OperationConflictDecision) {
    let LifecycleCommandOutcome::OperationAccepted(operation) = dispatch.outcome else {
        panic!("expected an accepted operation, got {:?}", dispatch.outcome);
    };
    (operation.returned_operation_id, operation.decision)
}

/// §8.3, `Stop (either)` + `Restart`: queue the restart. It waits in the
/// store until the stop finishes, and is then a start of an Inactive service
/// -- not a second stop leg against a service already Stopping.
#[test]
fn a_restart_queued_behind_a_stop_becomes_a_start_once_the_stop_completes() {
    let (mut supervisor, mut tokens, mut launcher) = booted_active_app(alive_service("app"));
    let main_job = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("main job")
        .id;
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 1,
        LIFECYCLE_COMMAND_NS + 2,
        LIFECYCLE_COMMAND_NS + 3,
    ]);

    let (stop_id, stop_decision) = accepted(
        supervisor
            .stop_service("app", None, &mut clock)
            .expect("stop app"),
    );
    assert_eq!(stop_decision, OperationConflictDecision::CreateNew);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop execution");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Stopping
    );

    let (restart_id, restart_decision) = accepted(
        supervisor
            .restart_service("app", None, &mut clock)
            .expect("restart app"),
    );
    assert_eq!(restart_decision, OperationConflictDecision::QueueNew);
    // Queued means not on the control boundary.
    assert!(supervisor.pending_control_operations().is_empty());
    assert!(!supervisor.has_ready_queued_operations());
    assert_eq!(
        supervisor
            .operation_status(restart_id)
            .expect("restart")
            .state,
        OperationState::Pending
    );

    // The stop completes: the service is Inactive and the restart is ready.
    supervisor
        .complete_job(main_job, LIFECYCLE_COMMAND_NS + 10, 0)
        .expect("main exits");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive
    );
    assert_eq!(
        supervisor.operation_status(stop_id).expect("stop").state,
        OperationState::Completed
    );
    assert!(supervisor.has_ready_queued_operations());

    let promoted = supervisor
        .promote_queued_operations(LIFECYCLE_COMMAND_NS + 11)
        .expect("promote");
    assert_eq!(promoted.len(), 1);
    assert_eq!(promoted[0].operation_id, restart_id);
    assert_eq!(promoted[0].outcome, PromotedOperationOutcome::StartPlan);
    assert_eq!(promoted[0].start_dispatches.len(), 1);
    assert!(!supervisor.has_ready_queued_operations());
    let starting = supervisor.service_status("app").expect("app");
    assert_eq!(starting.state, ServiceState::Starting);
    assert_eq!(
        starting.current_operation.expect("restart running").id,
        restart_id
    );

    let mut launch_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS + 20]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut launch_clock)
        .expect("launch restart")
        .expect("restart launch dispatch");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active
    );
    assert_eq!(
        supervisor
            .operation_status(restart_id)
            .expect("restart")
            .state,
        OperationState::Completed
    );
}

/// §8.3, `Restart (Running)` + `Stop`: abort the restart, create the stop.
/// The stop adopts the stop leg the restart already started; the main
/// process's exit completes the stop, and no start leg follows.
#[test]
fn a_stop_that_aborts_a_running_restart_adopts_its_stop_leg() {
    let (mut supervisor, _tokens, _launcher) = booted_active_app(alive_service("app"));
    let main_job = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("main job")
        .id;
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 1,
        LIFECYCLE_COMMAND_NS + 2,
        LIFECYCLE_COMMAND_NS + 3,
    ]);

    let (restart_id, _) = accepted(
        supervisor
            .restart_service("app", None, &mut clock)
            .expect("restart app"),
    );
    let leg = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute restart")
        .expect("restart stop leg");
    assert_eq!(leg.execution.kind, ControlOperationKind::RestartStopLeg);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Stopping
    );

    let (stop_id, stop_decision) = accepted(
        supervisor
            .stop_service("app", None, &mut clock)
            .expect("stop app"),
    );
    assert_eq!(
        stop_decision,
        OperationConflictDecision::AbortExistingThenCreate {
            existing_id: restart_id
        }
    );
    let stop = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop execution");
    assert_eq!(stop.execution.kind, ControlOperationKind::Stop);
    assert!(stop.execution.service_transition.is_none());
    assert!(matches!(
        stop.execution.detail,
        ControlExecutionDetail::StopAlreadyAcknowledged { .. }
    ));
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Stopping
    );

    supervisor
        .complete_job(main_job, LIFECYCLE_COMMAND_NS + 10, 0)
        .expect("main exits");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive
    );
    assert_eq!(
        supervisor.operation_status(stop_id).expect("stop").state,
        OperationState::Completed
    );
    assert_eq!(
        supervisor
            .operation_status(restart_id)
            .expect("restart")
            .state,
        OperationState::Aborted
    );
    assert!(supervisor.pending_launch_jobs().is_empty());
}

/// §8.3, `Reload (Running)` + `Stop`: abort the reload, create the stop. The
/// reload command's job is failed with the stop, so its terminal never
/// reports a completed reload against a service that is no longer Reloading.
#[test]
fn a_stop_that_aborts_a_running_reload_command_fails_the_commands_job() {
    let mut app = alive_service("app");
    app.exec_reload = Some("/bin/reload".to_string());
    let (mut supervisor, mut tokens, mut launcher) = booted_active_app(app);
    let main_job = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("main job")
        .id;
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 1,
        LIFECYCLE_COMMAND_NS + 2,
        LIFECYCLE_COMMAND_NS + 3,
        LIFECYCLE_COMMAND_NS + 4,
    ]);

    let (reload_id, _) = accepted(
        supervisor
            .reload_service("app", None, &mut clock)
            .expect("reload app"),
    );
    let reload = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload execution");
    let ControlExecutionDetail::ReloadCommand { job_id, .. } = reload.execution.detail else {
        panic!("expected a reload command");
    };
    supervisor
        .launch_next_pending_control_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch reload command")
        .expect("reload command launch");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Reloading
    );

    let (stop_id, stop_decision) = accepted(
        supervisor
            .stop_service("app", None, &mut clock)
            .expect("stop app"),
    );
    assert_eq!(
        stop_decision,
        OperationConflictDecision::AbortExistingThenCreate {
            existing_id: reload_id
        }
    );
    let stop = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute stop")
        .expect("stop execution");
    assert_eq!(stop.execution.cancelled_reload_jobs.len(), 1);
    assert_eq!(stop.execution.cancelled_reload_jobs[0].job_id, job_id);
    assert!(supervisor.jobs().get(job_id).is_none());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Stopping
    );

    // The reload command's process is reaped later; nothing routes it to a
    // reload completion, and the stop finishes as an ordinary stop.
    supervisor
        .complete_job(main_job, LIFECYCLE_COMMAND_NS + 10, 0)
        .expect("main exits");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive
    );
    assert_eq!(
        supervisor.operation_status(stop_id).expect("stop").state,
        OperationState::Completed
    );
    assert_eq!(
        supervisor
            .operation_status(reload_id)
            .expect("reload")
            .state,
        OperationState::Aborted
    );
}
