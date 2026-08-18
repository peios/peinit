mod completion;
mod creation;
mod failure;

use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    BOOT_NS, ScriptedClock, StaticRegistry, TestProcessLauncher, TestTokenProvider, alive_service,
    process, settings,
};

const PRE_HOOK_LAUNCH_NS: u64 = BOOT_NS + 10_000;
const PRE_HOOK_DONE_NS: u64 = BOOT_NS + 20_000;
const SECOND_PRE_HOOK_LAUNCH_NS: u64 = BOOT_NS + 30_000;
const SECOND_PRE_HOOK_DONE_NS: u64 = BOOT_NS + 40_000;
const MAIN_LAUNCH_NS: u64 = BOOT_NS + 50_000;

fn boot_app_with_single_pre_hook() -> (Supervisor, crate::ids::JobId, crate::ids::OperationId) {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry =
        StaticRegistry::services(vec![app_with_pre_hooks(vec!["/bin/pre".to_string()])]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    (
        supervisor,
        boot.start_dispatches[0].job_id,
        boot.start_dispatches[0].ready.operation_id,
    )
}

fn app_with_pre_hooks(commands: Vec<String>) -> crate::service::ServiceDefinition {
    let mut app = alive_service("app");
    app.exec_start_pre = commands;
    app.start_timeout_secs = 45;
    app
}

fn launch_start_hook(
    supervisor: &mut Supervisor,
    job_id: crate::ids::JobId,
    launched_at_ns: u64,
    pid: u32,
) {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, 71)]);
    let mut clock = ScriptedClock::new([launched_at_ns]);
    let dispatch = supervisor
        .launch_next_pending_start_hook_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch hook")
        .expect("hook launch dispatch");
    assert_eq!(dispatch.launch.job_id, job_id);
}
