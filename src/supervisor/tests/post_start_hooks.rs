use crate::control::query::ServiceStatusWarningType;
use crate::job::{JobEventDetail, JobType};
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;
use crate::supervisor::{Supervisor, SupervisorPostStartHookLaunchResult, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, oneshot_service, process, settings,
};

const POST_HOOK_LAUNCH_NS: u64 = BOOT_NS + 30_000;
const POST_HOOK_DONE_NS: u64 = BOOT_NS + 40_000;
const WORKER_LAUNCH_NS: u64 = BOOT_NS + 50_000;
const ONESHOT_DONE_NS: u64 = BOOT_NS + 20_000;
const POST_HOOK_HEALTH_INTERVAL_SECS: u64 = 7;
const POST_HOOK_WATCHDOG_SECS: u64 = 11;

#[test]
fn exec_start_post_delays_dependent_release_until_hook_completes() {
    let (mut supervisor, app_operation) = boot_app_with_worker_and_post_hook();
    let app_job = launch_app_and_get_post_hook(&mut supervisor);
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(supervisor.pending_post_hook_launch_jobs(), vec![app_job]);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor
            .operation_status(app_operation)
            .expect("app operation")
            .state,
        OperationState::Running,
    );

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6100, 31), process(6200, 32)]);
    let mut clock = ScriptedClock::new([POST_HOOK_LAUNCH_NS]);
    let mut controller = TestProcessController::default();
    let launch = supervisor
        .launch_next_pending_post_hook_job_with_controller(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut controller,
        )
        .expect("launch post hook")
        .expect("post hook launch");
    let SupervisorPostStartHookLaunchResult::Launched(launch) = launch else {
        panic!("expected successful post-hook launch");
    };
    assert_eq!(launch.launch.job_id, app_job);
    assert_eq!(
        supervisor.jobs().get(app_job).expect("post hook").job_type,
        JobType::PostExecHook,
    );

    let terminal = supervisor
        .complete_post_start_hook_job(app_job, POST_HOOK_DONE_NS, 0, &mut controller)
        .expect("complete post hook");

    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks".to_string()],
    );
    assert!(terminal.terminal.next_job_event.is_none());
    assert_eq!(
        terminal
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["worker"],
    );
    assert_eq!(
        supervisor.pending_launch_jobs(),
        terminal
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.job_id)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        supervisor
            .operation_status(app_operation)
            .expect("app operation")
            .result
            .as_deref(),
        Some("alive readiness: process started; ExecStartPost completed"),
    );

    let mut worker_clock = ScriptedClock::new([WORKER_LAUNCH_NS]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut worker_clock)
        .expect("launch worker")
        .expect("worker launch");
    assert_eq!(
        supervisor.service_status("worker").expect("worker").state,
        ServiceState::Active,
    );
}

#[test]
fn exec_start_post_failure_is_ignored_and_releases_dependents() {
    let (mut supervisor, app_operation) = boot_app_with_worker_and_post_hook();
    let app_job = launch_app_and_get_post_hook(&mut supervisor);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6100, 31)]);
    let mut clock = ScriptedClock::new([POST_HOOK_LAUNCH_NS]);
    let mut controller = TestProcessController::default();
    supervisor
        .launch_next_pending_post_hook_job_with_controller(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut controller,
        )
        .expect("launch post hook")
        .expect("post hook launch");

    let terminal = supervisor
        .complete_post_start_hook_job(app_job, POST_HOOK_DONE_NS, 2, &mut controller)
        .expect("complete failed post hook");

    assert!(matches!(
        terminal.terminal.job_event.detail,
        JobEventDetail::Ended {
            exit_code: Some(2),
            ..
        }
    ));
    assert_eq!(
        supervisor
            .operation_status(app_operation)
            .expect("app operation")
            .state,
        OperationState::Completed,
    );
    assert_eq!(
        supervisor
            .operation_status(app_operation)
            .expect("app operation")
            .result
            .as_deref(),
        Some("alive readiness: process started; ExecStartPost failure ignored"),
    );
    assert_eq!(
        terminal
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["worker"],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
}

#[test]
fn exec_start_post_delays_health_and_watchdog_until_hook_completes() {
    let mut supervisor = boot_app_with_post_hook_and_timers();
    let app_job = launch_app_and_get_post_hook(&mut supervisor);

    assert!(supervisor.next_health_check_interval().is_none());
    assert!(supervisor.next_watchdog_timeout().is_none());

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6100, 31)]);
    let mut clock = ScriptedClock::new([POST_HOOK_LAUNCH_NS]);
    let mut controller = TestProcessController::default();
    supervisor
        .launch_next_pending_post_hook_job_with_controller(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut controller,
        )
        .expect("launch post hook")
        .expect("post hook launch");

    supervisor
        .complete_post_start_hook_job(app_job, POST_HOOK_DONE_NS, 0, &mut controller)
        .expect("complete post hook");

    assert_eq!(
        supervisor
            .next_health_check_interval()
            .expect("health interval")
            .due_at_ns,
        POST_HOOK_DONE_NS + POST_HOOK_HEALTH_INTERVAL_SECS * 1_000_000_000,
    );
    assert_eq!(
        supervisor
            .next_watchdog_timeout()
            .expect("watchdog timeout")
            .due_at_ns,
        POST_HOOK_DONE_NS + POST_HOOK_WATCHDOG_SECS * 1_000_000_000,
    );
}

#[test]
fn exec_start_post_leaked_hooks_cgroup_is_projected_as_warning() {
    let (mut supervisor, _app_operation) = boot_app_with_worker_and_post_hook();
    let app_job = launch_app_and_get_post_hook(&mut supervisor);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6100, 31)]);
    let mut clock = ScriptedClock::new([POST_HOOK_LAUNCH_NS]);
    let mut controller = TestProcessController::default();
    supervisor
        .launch_next_pending_post_hook_job_with_controller(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut controller,
        )
        .expect("launch post hook")
        .expect("post hook launch");

    supervisor
        .complete_post_start_hook_job(app_job, POST_HOOK_DONE_NS, 0, &mut controller)
        .expect("complete post hook");
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/app/hooks", true);
    let cleanup_due = POST_HOOK_DONE_NS + 5_000_000_000;

    assert!(
        supervisor
            .process_due_cgroup_cleanups(&mut controller, cleanup_due)
            .expect("cleanup")
    );

    let status = supervisor.service_status("app").expect("app status");
    assert_eq!(status.warnings.len(), 1);
    assert_eq!(status.warnings[0].path, "/sys/fs/cgroup/peinit/app/hooks");
    assert_eq!(
        status.warnings[0].warning_type,
        ServiceStatusWarningType::Hooks
    );
    assert_eq!(status.warnings[0].detected_at_ns, cleanup_due);
}

#[test]
fn queued_exec_start_post_timeout_is_ignored_and_removed_from_launch_queue() {
    let (mut supervisor, app_operation) = boot_app_with_worker_and_post_hook();
    let app_job = launch_app_and_get_post_hook(&mut supervisor);
    let due_at_ns = supervisor
        .next_post_start_hook_timeout()
        .expect("post hook timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();

    let timeout = supervisor
        .process_next_due_post_start_hook_timeout(&mut controller, due_at_ns)
        .expect("process post hook timeout")
        .expect("timeout dispatch");

    assert_eq!(timeout.timeout.job_event.job_id, app_job);
    assert!(supervisor.pending_post_hook_launch_jobs().is_empty());
    assert!(supervisor.jobs().get(app_job).is_none());
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks".to_string()],
    );
    assert_eq!(
        supervisor
            .operation_status(app_operation)
            .expect("app operation")
            .result
            .as_deref(),
        Some("alive readiness: process started; ExecStartPost failure ignored"),
    );
    assert_eq!(
        timeout
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["worker"],
    );
}

#[test]
fn non_retained_oneshot_exec_start_post_completion_clears_after_dependent_release() {
    let (mut supervisor, task_operation) = boot_oneshot_with_worker_and_post_hook();
    let post_hook = launch_oneshot_and_get_post_hook(&mut supervisor);
    assert_eq!(
        supervisor.service_status("task").expect("task").state,
        ServiceState::Completed,
    );

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6100, 31)]);
    let mut clock = ScriptedClock::new([POST_HOOK_LAUNCH_NS]);
    let mut controller = TestProcessController::default();
    supervisor
        .launch_next_pending_post_hook_job_with_controller(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut controller,
        )
        .expect("launch post hook")
        .expect("post hook launch");

    let terminal = supervisor
        .complete_post_start_hook_job(post_hook, POST_HOOK_DONE_NS, 0, &mut controller)
        .expect("complete post hook");

    assert_eq!(
        supervisor.service_status("task").expect("task").state,
        ServiceState::Inactive,
    );
    assert_eq!(
        terminal
            .terminal
            .service_transitions
            .iter()
            .map(|transition| transition.event.to)
            .collect::<Vec<_>>(),
        vec![ServiceState::Inactive],
    );
    assert_eq!(
        terminal
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["worker"],
    );
    assert_eq!(
        supervisor
            .operation_status(task_operation)
            .expect("task operation")
            .state,
        OperationState::Completed,
    );
}

#[test]
fn non_retained_oneshot_exec_start_post_timeout_clears_after_dependent_release() {
    let (mut supervisor, task_operation) = boot_oneshot_with_worker_and_post_hook();
    let post_hook = launch_oneshot_and_get_post_hook(&mut supervisor);
    let due_at_ns = supervisor
        .next_post_start_hook_timeout()
        .expect("post hook timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();

    let timeout = supervisor
        .process_next_due_post_start_hook_timeout(&mut controller, due_at_ns)
        .expect("process post hook timeout")
        .expect("timeout dispatch");

    assert_eq!(timeout.timeout.job_event.job_id, post_hook);
    assert_eq!(
        supervisor.service_status("task").expect("task").state,
        ServiceState::Inactive,
    );
    assert_eq!(
        timeout
            .timeout
            .service_transitions
            .iter()
            .map(|transition| transition.event.to)
            .collect::<Vec<_>>(),
        vec![ServiceState::Inactive],
    );
    assert_eq!(
        timeout
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["worker"],
    );
    assert_eq!(
        supervisor
            .operation_status(task_operation)
            .expect("task operation")
            .state,
        OperationState::Completed,
    );
}

fn boot_app_with_worker_and_post_hook() -> (Supervisor, crate::ids::OperationId) {
    let mut app = alive_service("app");
    app.exec_start_post = vec!["/usr/bin/post --flag".to_string()];
    app.start_timeout_secs = 45;
    let mut worker = alive_service("worker");
    worker.requires.push("app".to_string());

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, worker]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    assert_eq!(
        boot.start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    let app_operation = boot.start_dispatches[0].ready.operation_id;
    (supervisor, app_operation)
}

fn boot_oneshot_with_worker_and_post_hook() -> (Supervisor, crate::ids::OperationId) {
    let mut task = oneshot_service("task");
    task.exec_start_post = vec!["/usr/bin/post --flag".to_string()];
    let mut worker = alive_service("worker");
    worker.requires.push("task".to_string());

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![task, worker]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    assert_eq!(
        boot.start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["task"],
    );
    let task_operation = boot.start_dispatches[0].ready.operation_id;
    (supervisor, task_operation)
}

fn boot_app_with_post_hook_and_timers() -> Supervisor {
    let mut app = alive_service("app");
    app.exec_start_post = vec!["/usr/bin/post --flag".to_string()];
    app.health_check = Some("/usr/bin/app-health".to_string());
    app.health_check_interval_secs = POST_HOOK_HEALTH_INTERVAL_SECS;
    app.watchdog_timeout_secs = POST_HOOK_WATCHDOG_SECS;

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    assert_eq!(boot.start_dispatches.len(), 1);
    supervisor
}

fn launch_app_and_get_post_hook(supervisor: &mut Supervisor) -> crate::ids::JobId {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6000, 30)]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);
    let launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");
    let post_hook = launch.started.post_start_hook.expect("post hook created");
    assert!(launch.start_dispatches.is_empty());
    assert!(matches!(post_hook.detail, JobEventDetail::Created { .. }));
    assert_eq!(post_hook.job_type, JobType::PostExecHook);
    post_hook.job_id
}

fn launch_oneshot_and_get_post_hook(supervisor: &mut Supervisor) -> crate::ids::JobId {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6000, 30)]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch task")
        .expect("task launch");
    let task_job = supervisor
        .service_status("task")
        .expect("task")
        .current_job
        .expect("task job")
        .id;

    let terminal = supervisor
        .complete_job(task_job, ONESHOT_DONE_NS, 0)
        .expect("complete task");
    let post_hook = terminal
        .terminal
        .post_start_hook
        .expect("post hook created");
    assert!(terminal.start_dispatches.is_empty());
    assert_eq!(post_hook.job_type, JobType::PostExecHook);
    post_hook.job_id
}
