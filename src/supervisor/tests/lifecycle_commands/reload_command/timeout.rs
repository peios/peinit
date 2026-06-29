use crate::operation::{OperationState, operation_timeout_result};
use crate::service::runtime::ServiceState;

use super::*;

#[test]
fn reload_command_timeout_kills_hooks_cgroup_and_fails_operation() {
    let mut supervisor = active_app_supervisor_with_reload_command();
    let operation_id = reload_app(&mut supervisor);
    let job_id = execute_and_launch_reload_command(&mut supervisor);
    let mut controller = TestProcessController::default();
    let timeout_ns = CONTROL_NS + 45_000_000_000;

    let timeout = supervisor
        .process_next_due_reload_command_timeout(&mut controller, timeout_ns)
        .expect("process timeout")
        .expect("timeout dispatch");

    assert_eq!(timeout.timeout.job_event.job_id, job_id);
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks".to_string()],
    );
    assert!(supervisor.jobs().get(job_id).is_none());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    let operation = supervisor
        .operation_status(operation_id)
        .expect("reload operation");
    assert_eq!(operation.state, OperationState::Failed);
    let expected = operation_timeout_result("ExecReload command timed out");
    assert_eq!(operation.error.as_deref(), Some(expected.as_str()));
}
