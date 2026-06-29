use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::operation::{OperationState, is_operation_timeout_result};
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, oneshot_service, process, settings,
};

const START_TIMEOUT_SECS: u64 = 1;
const START_TIMEOUT_NS: u64 = START_TIMEOUT_SECS * 1_000_000_000;
const TASK_ROOT_CGROUP: &str = "/sys/fs/cgroup/peinit/task";

#[test]
fn launched_oneshot_start_operation_timeout_kills_service_root_and_fails_operation() {
    let mut supervisor = task_supervisor();
    let mut command_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let operation_id = start_task(&mut supervisor, &mut command_clock);

    let mut launch_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS + 1]);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8100, 51)]);
    let launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut launch_clock)
        .expect("launch task")
        .expect("task launch dispatch");
    let job_id = launch.started.job_event.job_id;

    let mut controller = TestProcessController::default();
    let turn = supervisor
        .process_due_operation_maintenance_with_controller(
            &mut controller,
            LIFECYCLE_COMMAND_NS + START_TIMEOUT_NS,
        )
        .expect("operation maintenance");

    assert_eq!(turn.service_main_start_timeouts.len(), 1);
    let timeout = &turn.service_main_start_timeouts[0];
    assert_eq!(timeout.job_event.job_id, job_id);
    assert_eq!(timeout.killed_cgroup_id.as_deref(), Some(TASK_ROOT_CGROUP));
    assert_eq!(controller.cgroup_kills, vec![TASK_ROOT_CGROUP.to_string()]);
    assert_operation_timed_out(&supervisor, operation_id);
    assert_task_backoff_without_current_job(&supervisor);
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert!(supervisor.jobs().get(job_id).is_none());
}

#[test]
fn unlaunched_oneshot_start_operation_timeout_drops_pending_launch_without_kill() {
    let mut supervisor = task_supervisor();
    let mut command_clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let operation_id = start_task(&mut supervisor, &mut command_clock);
    let job_id = supervisor
        .service_status("task")
        .expect("task status")
        .current_job
        .expect("created task job")
        .id;

    let mut controller = TestProcessController::default();
    let turn = supervisor
        .process_due_operation_maintenance_with_controller(
            &mut controller,
            LIFECYCLE_COMMAND_NS + START_TIMEOUT_NS,
        )
        .expect("operation maintenance");

    assert_eq!(turn.service_main_start_timeouts.len(), 1);
    let timeout = &turn.service_main_start_timeouts[0];
    assert_eq!(timeout.job_event.job_id, job_id);
    assert_eq!(timeout.killed_cgroup_id, None);
    assert!(controller.cgroup_kills.is_empty());
    assert_operation_timed_out(&supervisor, operation_id);
    assert_task_backoff_without_current_job(&supervisor);
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert!(supervisor.jobs().get(job_id).is_none());
}

fn task_supervisor() -> Supervisor {
    let mut task = oneshot_service("task");
    task.triggers.clear();
    task.start_timeout_secs = START_TIMEOUT_SECS;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![task]);
    let mut boot_clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut boot_clock)
        .expect("boot supervisor");
    supervisor
}

fn start_task(supervisor: &mut Supervisor, clock: &mut ScriptedClock) -> crate::ids::OperationId {
    let dispatch = supervisor
        .start_service("task", None, clock)
        .expect("start task");
    match dispatch.outcome {
        LifecycleCommandOutcome::OperationAccepted(operation) => operation.returned_operation_id,
        LifecycleCommandOutcome::OnDemandStart(dispatch) => {
            dispatch.requested_operation.returned_operation_id
        }
        other => {
            panic!("expected start operation, got {other:?}");
        }
    }
}

fn assert_operation_timed_out(supervisor: &Supervisor, operation_id: crate::ids::OperationId) {
    let status = supervisor
        .operation_status(operation_id)
        .expect("operation status");
    assert_eq!(status.state, OperationState::Failed);
    let error = status.error.as_deref().expect("operation error");
    assert!(is_operation_timeout_result(error));
}

fn assert_task_backoff_without_current_job(supervisor: &Supervisor) {
    let status = supervisor.service_status("task").expect("task status");
    assert_eq!(status.state, ServiceState::Backoff);
    assert_eq!(status.cause, Some(TransitionCause::ReadinessTimeout));
    assert!(status.current_job.is_none());
}
