//! PEI-803: a control operation the boundary cannot begin fails the operation,
//! not the runtime loop.
//!
//! `begin_control_operation` refuses an operation whose service has no current
//! main job, no running job, or no definition. Each of those is a fault in
//! peinit's own bookkeeping — the matrix admitted something execution cannot
//! do — and each escaped `execute_next_pending_control_operation` as a
//! `SupervisorError`, which the work pump turned into a fatal loop error. PID 1
//! then entered recovery and unlinked its sockets: one administrator's command
//! cost the machine its control interface.

use crate::execution::control::ControlExecutionError;
use crate::operation::store::OperationRequest;
use crate::operation::{OperationSource, OperationState, OperationType, is_internal_error_result};
use crate::runtime::{RuntimeWorkPumpConfig, RuntimeWorkPumpContext, drain_runtime_work_queues};
use crate::service::runtime::ServiceState;
use crate::supervisor::tests::{
    LIFECYCLE_COMMAND_NS, ScriptedClock, TestFilesystemCheckLauncher, TestProcessController,
    TestProcessLauncher, TestTokenProvider,
};
use crate::supervisor::{PendingControlOperation, PendingControlRequirement, Supervisor};

use super::super::{active_app_supervisor, booted_supervisor};
use super::{CONTROL_NS, current_app_job, stop_app};

/// Stage the fault directly: a Pending Stop for a service that has no main
/// job, queued for the boundary as if admission had let it through.
fn stage_stop_without_a_main_job(supervisor: &mut Supervisor) -> crate::ids::OperationId {
    let operation_id = supervisor
        .operation_ids
        .allocate_batch(1, LIFECYCLE_COMMAND_NS)
        .expect("operation id")[0];
    supervisor
        .operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Stop,
            service: "app".to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: LIFECYCLE_COMMAND_NS,
        })
        .expect("request stop");
    supervisor
        .pending_control_operations
        .push_back(PendingControlOperation {
            operation_id,
            service: "app".to_string(),
            operation_type: OperationType::Stop,
            requirement: PendingControlRequirement::StopProcess,
        });
    operation_id
}

#[test]
fn a_control_operation_the_boundary_cannot_begin_fails_the_operation_only() {
    let mut app = crate::supervisor::tests::alive_service("app");
    app.triggers.clear();
    let mut supervisor = booted_supervisor(vec![app]);
    let operation_id = stage_stop_without_a_main_job(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    let executed = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("the boundary's refusal is not a supervisor error");

    // Nothing executed, nothing was signalled, and the queue is drained.
    assert!(executed.is_none());
    assert!(controller.signals.is_empty());
    assert!(supervisor.pending_control_operations().is_empty());
    // The operation failed with the internal-error result, so a client that
    // asks after it — or waits on it — learns that peinit, not the service,
    // is what went wrong.
    let failed = supervisor
        .operation_status(operation_id)
        .expect("failed operation");
    assert_eq!(failed.state, OperationState::Failed);
    assert!(
        failed
            .error
            .as_deref()
            .is_some_and(is_internal_error_result),
        "result names the class: {:?}",
        failed.error
    );
    assert!(
        failed
            .error
            .as_deref()
            .is_some_and(|error| error.contains("MissingCurrentMainJob")),
        "and the cause: {:?}",
        failed.error
    );
    // The service kept the state it had.
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive
    );
    // And the failure is reported once, with the boundary's own error, for
    // the console and the event stream.
    let failures = supervisor.take_control_operation_failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].operation_id, operation_id);
    assert_eq!(failures[0].service, "app");
    assert_eq!(failures[0].operation_type, OperationType::Stop);
    assert_eq!(
        failures[0].error,
        ControlExecutionError::MissingCurrentMainJob {
            service: "app".to_string()
        }
    );
    assert!(supervisor.take_control_operation_failures().is_empty());
}

#[test]
fn the_work_pump_reports_a_contained_control_failure_and_keeps_going() {
    let mut app = crate::supervisor::tests::alive_service("app");
    app.triggers.clear();
    let mut supervisor = booted_supervisor(vec![app]);
    let operation_id = stage_stop_without_a_main_job(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS, CONTROL_NS + 1, CONTROL_NS + 2]);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(Vec::new());
    let mut filesystem_check_launcher = TestFilesystemCheckLauncher::default();

    let turn = drain_runtime_work_queues(
        &mut supervisor,
        &mut RuntimeWorkPumpContext {
            clock: &mut clock,
            controller: &mut controller,
            token_provider: &mut tokens,
            process_launcher: &mut launcher,
            filesystem_check_launcher: &mut filesystem_check_launcher,
            config: RuntimeWorkPumpConfig::default(),
        },
    )
    .expect("the pump survives a contained control failure");

    assert!(turn.control_operations.is_empty());
    assert_eq!(turn.control_operation_failures.len(), 1);
    assert_eq!(turn.control_operation_failures[0].operation_id, operation_id);
    // A failed operation is not a stale queue entry: it was live and refused.
    assert_eq!(turn.stale_control_operations, 0);
    assert!(supervisor.pending_control_operations().is_empty());
}

/// The job vanishes between admission and execution: the stop was admitted
/// against an Active service whose job then finished under it. The boundary
/// finds no current main job, and that is contained the same way.
#[test]
fn a_stop_whose_job_finished_before_the_boundary_ran_fails_the_operation_only() {
    let mut supervisor = active_app_supervisor();
    let operation_id = stop_app(&mut supervisor);
    let job = current_app_job(&supervisor);
    supervisor
        .jobs_mut()
        .complete_job(job, CONTROL_NS - 1, 0)
        .expect("finish the job under the pending stop");
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    let executed = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("contained");

    assert!(executed.is_none());
    assert!(controller.signals.is_empty());
    let failed = supervisor
        .operation_status(operation_id)
        .expect("failed operation");
    assert_eq!(failed.state, OperationState::Failed);
    assert!(
        failed
            .error
            .as_deref()
            .is_some_and(|error| error.contains("MissingCurrentMainJob")),
        "{:?}",
        failed.error
    );
    let failures = supervisor.take_control_operation_failures();
    assert_eq!(failures.len(), 1);
    assert!(matches!(
        failures[0].error,
        ControlExecutionError::MissingCurrentMainJob { .. }
    ));
}
