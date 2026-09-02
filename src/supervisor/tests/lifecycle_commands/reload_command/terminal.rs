use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::job::JobState;
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;
use crate::shutdown::ShutdownKind;

use super::*;

#[test]
fn successful_reload_command_completes_operation_and_returns_active() {
    let mut supervisor = active_app_supervisor_with_reload_command();
    let operation_id = reload_app(&mut supervisor);
    let job_id = execute_and_launch_reload_command(&mut supervisor);
    let mut controller = TestProcessController::default();

    let terminal = supervisor
        .complete_reload_command_job(job_id, RELOAD_COMMAND_EXIT_NS, 0, &mut controller)
        .expect("complete reload command");

    assert_eq!(terminal.terminal.job_event.job_id, job_id);
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks"]
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    let operation = supervisor
        .operation_status(operation_id)
        .expect("reload operation");
    assert_eq!(operation.state, OperationState::Completed);
    assert_eq!(operation.result.as_deref(), Some("reload command advisory"));
}

#[test]
fn failing_reload_command_fails_operation_but_service_returns_active() {
    let mut supervisor = active_app_supervisor_with_reload_command();
    let operation_id = reload_app(&mut supervisor);
    let job_id = execute_and_launch_reload_command(&mut supervisor);
    let mut controller = TestProcessController::default();

    supervisor
        .complete_reload_command_job(job_id, RELOAD_COMMAND_EXIT_NS, 2, &mut controller)
        .expect("fail reload command");

    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks"]
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    let operation = supervisor
        .operation_status(operation_id)
        .expect("reload operation");
    assert_eq!(operation.state, OperationState::Failed);
    assert_eq!(
        operation.error.as_deref(),
        Some("ExecReload command failed (exit 2)"),
    );
}

/// PEI-605. The reload command job is cancelled while it is still `Created`
/// and therefore still queued for launch, so the queue entry has to go with
/// it. The drain tolerates a stale entry now, but leaving one behind means the
/// next control launch spends itself working through rubbish instead of doing
/// the work it was called for.
#[test]
fn a_main_crash_before_the_reload_hook_launches_clears_its_queue_entry() {
    let mut supervisor = active_app_supervisor_with_reload_command();
    reload_app(&mut supervisor);
    let job_id = execute_reload_command(&mut supervisor);
    assert_eq!(supervisor.pending_control_launch_jobs(), vec![job_id]);
    assert_eq!(
        supervisor.jobs().get(job_id).expect("reload hook").state,
        JobState::Created,
        "the hook must still be queued and unlaunched — that is the premise",
    );
    let main_job = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("main job")
        .id;
    let mut controller = TestProcessController::default();
    let mut finalizer = NoopFinalizer;

    supervisor
        .complete_job_with_runtime_context(
            main_job,
            RELOAD_COMMAND_EXIT_NS,
            1,
            &mut controller,
            &mut finalizer,
        )
        .expect("complete crashed main job");

    assert!(supervisor.jobs().get(job_id).is_none(), "record is gone");
    assert!(
        supervisor.pending_control_launch_jobs().is_empty(),
        "and so is its queue entry",
    );
    assert_eq!(
        supervisor.take_stale_launch_entries(),
        0,
        "nothing stale was left for the drain to clean up after",
    );
}

#[test]
fn main_process_crash_during_reload_command_kills_hook_and_fails_reload_operation() {
    let mut supervisor = active_app_supervisor_with_reload_command();
    let operation_id = reload_app(&mut supervisor);
    let job_id = execute_and_launch_reload_command(&mut supervisor);
    let main_job = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("main job")
        .id;
    let mut controller = TestProcessController::default();
    let mut finalizer = NoopFinalizer;

    let dispatch = supervisor
        .complete_job_with_runtime_context(
            main_job,
            RELOAD_COMMAND_EXIT_NS,
            1,
            &mut controller,
            &mut finalizer,
        )
        .expect("complete crashed main job");

    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks".to_string()],
    );
    assert!(supervisor.next_reload_command_timeout().is_none());
    assert!(supervisor.jobs().get(job_id).is_none());
    assert_ne!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Reloading,
    );
    let operation = supervisor
        .operation_status(operation_id)
        .expect("reload operation");
    assert_eq!(operation.state, OperationState::Failed);
    assert_eq!(
        operation.error.as_deref(),
        Some("reload failed: main process exited during reload"),
    );
    assert_eq!(dispatch.cleanup_job_events.len(), 1);
    assert_eq!(dispatch.cleanup_job_events[0].job_id, job_id);
    assert_eq!(dispatch.cleanup_job_events[0].state, JobState::Failed);
    assert_eq!(dispatch.terminal.operation_events.len(), 1);
    assert_eq!(
        dispatch.terminal.operation_events[0].operation_id,
        operation_id,
    );
}

struct NoopFinalizer;

impl ShutdownFinalizer for NoopFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        Ok(Vec::new())
    }

    fn unmount(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn remount_readonly(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn reboot(&mut self, _kind: ShutdownKind) -> Result<(), BoundaryError> {
        Ok(())
    }
}
