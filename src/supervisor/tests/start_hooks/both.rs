//! PEI-801: a service declaring both `ExecStartPre` and `ExecStartPost` runs
//! its post-hooks after readiness, exactly as a service with post-hooks alone
//! does (§5.3).

use crate::job::{JobEventDetail, JobType};
use crate::service::runtime::ServiceState;

use super::super::TestProcessController;
use super::*;

#[test]
fn a_service_with_pre_and_post_hooks_runs_its_post_hooks_after_readiness() {
    let mut app = app_with_pre_hooks(vec!["/bin/pre".to_string()]);
    app.exec_start_post = vec!["/bin/post --flag".to_string()];
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let hook = boot.start_dispatches[0].job_id;

    launch_start_hook(&mut supervisor, hook, PRE_HOOK_LAUNCH_NS, 6100);
    let mut controller = TestProcessController::default();
    let terminal = supervisor
        .complete_pre_start_hook_job(hook, PRE_HOOK_DONE_NS, 0, &mut controller)
        .expect("complete the pre-hook");
    let main_job = terminal
        .terminal
        .next_job_event
        .expect("main job created after the pre-hook");
    assert_eq!(main_job.job_type, JobType::ServiceMain);
    assert_eq!(supervisor.pending_launch_jobs(), vec![main_job.job_id]);

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6200, 72)]);
    let mut launch_clock = ScriptedClock::new([MAIN_LAUNCH_NS]);
    let launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut launch_clock)
        .expect("launch app")
        .expect("app launch dispatch");

    // Alive readiness: the service is Active, and the post-hook job exists
    // and is queued for launch, as it is for a service with no pre-hooks.
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active
    );
    let post_hook = launch
        .started
        .post_start_hook
        .expect("the post-hook job was created");
    assert_eq!(post_hook.job_type, JobType::PostExecHook);
    assert!(matches!(post_hook.detail, JobEventDetail::Created { .. }));
    assert_eq!(
        supervisor.pending_post_hook_launch_jobs(),
        vec![post_hook.job_id]
    );
    let job = supervisor.jobs().get(post_hook.job_id).expect("post-hook job");
    assert_eq!(job.image_path, "/bin/post");
    assert_eq!(job.cgroup_id, "/sys/fs/cgroup/peinit/app/hooks");
}
