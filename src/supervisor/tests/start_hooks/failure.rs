use crate::operation::{OperationState, operation_timeout_result};
use crate::service::runtime::{ServiceState, TransitionCause};

use super::super::TestProcessController;
use super::*;

#[test]
fn failing_pre_hook_kills_service_cgroup_and_fails_start() {
    let (mut supervisor, hook_job, operation_id) = boot_app_with_single_pre_hook();
    launch_start_hook(&mut supervisor, hook_job, PRE_HOOK_LAUNCH_NS, 6100);
    let mut controller = TestProcessController::default();

    supervisor
        .complete_pre_start_hook_job(hook_job, PRE_HOOK_DONE_NS, 2, &mut controller)
        .expect("complete failed hook");

    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert!(supervisor.pending_start_hook_launch_jobs().is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Backoff);
    assert_eq!(status.cause, Some(TransitionCause::PreHookFailure));
    let operation = supervisor
        .operation_status(operation_id)
        .expect("operation");
    assert_eq!(operation.state, OperationState::Failed);
    assert_eq!(
        operation.error.as_deref(),
        Some("ExecStartPre command failed (exit 2)"),
    );
}

#[test]
fn pre_hook_timeout_kills_service_cgroup_and_fails_start() {
    let (mut supervisor, hook_job, operation_id) = boot_app_with_single_pre_hook();
    launch_start_hook(&mut supervisor, hook_job, PRE_HOOK_LAUNCH_NS, 6100);
    let mut controller = TestProcessController::default();
    let due_at_ns = supervisor
        .next_pre_start_hook_timeout()
        .expect("hook timeout")
        .due_at_ns;

    let timeout = supervisor
        .process_next_due_pre_start_hook_timeout(&mut controller, due_at_ns)
        .expect("process timeout")
        .expect("timeout dispatch");

    assert_eq!(timeout.timeout.job_event.job_id, hook_job);
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert!(supervisor.jobs().get(hook_job).is_none());
    assert!(supervisor.next_pre_start_hook_timeout().is_none());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Backoff,
    );
    let operation = supervisor
        .operation_status(operation_id)
        .expect("operation");
    assert_eq!(operation.state, OperationState::Failed);
    let expected = operation_timeout_result("ExecStartPre command timed out");
    assert_eq!(operation.error.as_deref(), Some(expected.as_str()));
}
