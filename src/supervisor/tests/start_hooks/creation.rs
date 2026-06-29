use crate::boundary::BoundaryError;
use crate::execution::start::StartExecutionJobKind;
use crate::job::{JobEventDetail, JobType};
use crate::operation::OperationState;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::super::TestProcessController;
use super::*;

#[test]
fn start_with_exec_start_pre_creates_first_hook_job_in_hooks_cgroup() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app_with_pre_hooks(vec![
        r#"/usr/bin/pre --name="hello world""#.to_string(),
    ])]);
    let mut clock = ScriptedClock::new([BOOT_NS]);

    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    assert_eq!(boot.start_dispatches.len(), 1);
    let dispatch = &boot.start_dispatches[0];
    let StartExecutionJobKind::PreStartHook { main_job_id } = dispatch.job_kind else {
        panic!("expected pre-start hook dispatch");
    };
    assert!(matches!(
        dispatch.job_event.detail,
        JobEventDetail::Created { .. }
    ));
    assert_eq!(
        supervisor.pending_start_hook_launch_jobs(),
        vec![dispatch.job_id]
    );
    assert!(supervisor.pending_launch_jobs().is_empty());

    let hook = supervisor.jobs().get(dispatch.job_id).expect("hook job");
    assert_eq!(hook.job_type, JobType::PreExecHook);
    assert_eq!(hook.image_path, "/usr/bin/pre");
    assert_eq!(hook.arguments, vec!["--name=hello world"]);
    assert_eq!(hook.cgroup_id, "/sys/fs/cgroup/peinit/app/hooks");
    assert!(supervisor.jobs().get(main_job_id).is_none());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(
        supervisor
            .operation_status(dispatch.ready.operation_id)
            .expect("operation")
            .state,
        OperationState::Running,
    );
}

#[test]
fn exec_start_pre_uses_hook_identity_when_configured() {
    let mut app = app_with_pre_hooks(vec!["/usr/bin/pre".to_string()]);
    app.identity = "LocalService".to_string();
    app.hook_identity = Some("SYSTEM".to_string());
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);

    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let hook = supervisor
        .jobs()
        .get(boot.start_dispatches[0].job_id)
        .expect("hook job");
    assert_eq!(hook.job_type, JobType::PreExecHook);
    assert_eq!(hook.resolved_identity, "SYSTEM");
    assert_eq!(hook.token_summary.identity, "SYSTEM");
}

#[test]
fn start_hook_launch_uses_start_hook_queue() {
    let (mut supervisor, hook_job, _) = boot_app_with_single_pre_hook();
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6100, 71)]);
    let mut clock = ScriptedClock::new([PRE_HOOK_LAUNCH_NS]);

    let launch = supervisor
        .launch_next_pending_start_hook_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch hook")
        .expect("hook launch dispatch");

    assert_eq!(launch.launch.job_id, hook_job);
    assert_eq!(launch.launch.process.pid, 6100);
    assert!(supervisor.pending_start_hook_launch_jobs().is_empty());
    assert_eq!(
        launcher.observed_notify_sockets,
        vec![Some(
            SupervisorSettings::DEFAULT_NOTIFY_SOCKET_PATH.to_string()
        )],
    );
    assert_eq!(
        supervisor.jobs().get(hook_job).expect("hook").pid,
        Some(6100),
    );
}

#[test]
fn start_hook_launch_failure_fails_hook_and_start_operation() {
    let (mut supervisor, hook_job, operation_id) = boot_app_with_single_pre_hook();
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::results(vec![Err(BoundaryError::Process(
        "clone3 EAGAIN".to_string(),
    ))]);
    let mut clock = ScriptedClock::new([PRE_HOOK_LAUNCH_NS]);
    let mut controller = TestProcessController::default();

    let result = supervisor
        .launch_next_pending_start_hook_job_with_controller(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut controller,
        )
        .expect("handle hook launch failure")
        .expect("hook launch failure");
    let crate::supervisor::SupervisorStartHookLaunchResult::Failed(failure) = result else {
        panic!("expected hook launch failure");
    };

    assert_eq!(failure.job_event.job_id, hook_job);
    assert_eq!(failure.killed_cgroup_id, "/sys/fs/cgroup/peinit/app",);
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert!(supervisor.pending_start_hook_launch_jobs().is_empty());
    assert!(supervisor.jobs().get(hook_job).is_none());
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Backoff);
    assert_eq!(status.cause, Some(TransitionCause::PreHookFailure));
    let operation = supervisor
        .operation_status(operation_id)
        .expect("operation");
    assert_eq!(operation.state, OperationState::Failed);
    assert_eq!(
        operation.error.as_deref(),
        Some("PreHookFailure: launch failed: clone3 EAGAIN"),
    );
}
