use crate::execution::control::{ControlExecutionDetail, ControlOperationKind};
use crate::job::{JobEventDetail, JobType};
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;
use crate::supervisor::{SupervisorControlLaunchResult, SupervisorSettings};

use super::*;

#[test]
fn external_exec_reload_creates_reload_hook_job_in_hooks_cgroup() {
    let mut supervisor = active_app_supervisor_with_reload_command();
    let operation_id = reload_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    let dispatch = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload command dispatch");

    assert_eq!(dispatch.execution.kind, ControlOperationKind::ReloadCommand);
    assert!(controller.signals.is_empty());
    let ControlExecutionDetail::ReloadCommand { job_id, job_event } = dispatch.execution.detail
    else {
        panic!("expected reload command job");
    };
    assert_eq!(job_event.job_type, JobType::ReloadHook);
    assert!(matches!(job_event.detail, JobEventDetail::Created { .. }));
    assert_eq!(supervisor.pending_control_launch_jobs(), vec![job_id]);

    let job = supervisor.jobs().get(job_id).expect("reload hook job");
    assert_eq!(job.service.as_deref(), Some("app"));
    assert_eq!(job.job_type, JobType::ReloadHook);
    assert_eq!(job.image_path, "/usr/bin/reload");
    assert_eq!(job.arguments, vec!["--name=hello world", ""]);
    assert_eq!(job.cgroup_id, "/sys/fs/cgroup/peinit/app/hooks");
    assert_eq!(job.operation_id, Some(operation_id));
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Reloading,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("reload operation")
            .state,
        OperationState::Running,
    );
}

#[test]
fn reload_hook_launch_uses_control_launch_queue() {
    let mut supervisor = active_app_supervisor_with_reload_command();
    reload_app(&mut supervisor);
    let job_id = execute_reload_command(&mut supervisor);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9100, 61)]);
    let mut clock = ScriptedClock::new([RELOAD_COMMAND_LAUNCH_NS]);

    let dispatch = supervisor
        .launch_next_pending_control_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch control job")
        .expect("reload command launch");
    let SupervisorControlLaunchResult::Launched(dispatch) = dispatch else {
        panic!("expected reload command launch");
    };

    assert_eq!(dispatch.launch.job_id, job_id);
    assert_eq!(dispatch.launch.process.pid, 9100);
    assert!(supervisor.pending_control_launch_jobs().is_empty());
    assert_eq!(
        launcher.observed_notify_sockets,
        vec![Some(
            SupervisorSettings::DEFAULT_NOTIFY_SOCKET_PATH.to_string()
        )],
    );
    assert_eq!(
        supervisor.jobs().get(job_id).expect("reload hook").pid,
        Some(9100),
    );
}
