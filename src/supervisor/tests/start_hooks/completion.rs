use crate::job::JobType;
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;

use super::super::TestProcessController;
use super::*;

#[test]
fn successful_pre_hooks_run_sequentially_then_queue_main_job() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut app = app_with_pre_hooks(vec![
        "/usr/bin/pre-one".to_string(),
        "/usr/bin/pre-two --flag".to_string(),
    ]);
    app.identity = "LocalService".to_string();
    app.hook_identity = Some("SYSTEM".to_string());
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let operation_id = boot.start_dispatches[0].ready.operation_id;
    let first_hook = boot.start_dispatches[0].job_id;
    assert_eq!(
        supervisor
            .jobs()
            .get(first_hook)
            .expect("first hook")
            .resolved_identity,
        "SYSTEM",
    );

    launch_start_hook(&mut supervisor, first_hook, PRE_HOOK_LAUNCH_NS, 6100);
    let mut controller = TestProcessController::default();
    let first_terminal = supervisor
        .complete_pre_start_hook_job(first_hook, PRE_HOOK_DONE_NS, 0, &mut controller)
        .expect("complete first hook");
    let second_hook_event = first_terminal
        .terminal
        .next_job_event
        .expect("second hook created");
    assert_eq!(second_hook_event.job_type, JobType::PreExecHook);
    assert_eq!(second_hook_event.resolved_identity, "SYSTEM");
    let second_hook = second_hook_event.job_id;
    assert_eq!(
        supervisor.pending_start_hook_launch_jobs(),
        vec![second_hook]
    );
    assert!(controller.cgroup_kills.is_empty());

    launch_start_hook(
        &mut supervisor,
        second_hook,
        SECOND_PRE_HOOK_LAUNCH_NS,
        6101,
    );
    let second_terminal = supervisor
        .complete_pre_start_hook_job(second_hook, SECOND_PRE_HOOK_DONE_NS, 0, &mut controller)
        .expect("complete second hook");
    let main_job_event = second_terminal
        .terminal
        .next_job_event
        .expect("main job created");
    assert_eq!(main_job_event.job_type, JobType::ServiceMain);
    assert_eq!(main_job_event.resolved_identity, "LocalService");
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks".to_string()],
    );
    assert!(supervisor.pending_start_hook_launch_jobs().is_empty());
    assert_eq!(
        supervisor.pending_launch_jobs(),
        vec![main_job_event.job_id]
    );

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6200, 72)]);
    let mut clock = ScriptedClock::new([MAIN_LAUNCH_NS]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch main")
        .expect("main launch dispatch");

    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("operation")
            .state,
        OperationState::Completed,
    );
}
