mod creation;
mod signal;
mod terminal;
mod timeout;

use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::execution::control::ControlExecutionDetail;
use crate::supervisor::{Supervisor, SupervisorSettings};

use crate::supervisor::tests::{
    APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, alive_service, process,
    settings,
};

const CONTROL_NS: u64 = LIFECYCLE_COMMAND_NS + 1;
const RELOAD_COMMAND_LAUNCH_NS: u64 = LIFECYCLE_COMMAND_NS + 2;
const RELOAD_COMMAND_EXIT_NS: u64 = LIFECYCLE_COMMAND_NS + 3;

fn active_app_supervisor_with_reload_command() -> Supervisor {
    let mut app = alive_service("app");
    app.exec_reload = Some(r#"/bin/reload --name="hello world" """#.to_string());
    app.start_timeout_secs = 45;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot app");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    supervisor
}

fn reload_app(supervisor: &mut Supervisor) -> crate::ids::OperationId {
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .reload_service("app", None, &mut clock)
        .expect("reload app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = accepted.outcome else {
        panic!("expected reload operation");
    };
    operation.returned_operation_id
}

fn execute_reload_command(supervisor: &mut Supervisor) -> crate::ids::JobId {
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);
    let dispatch = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload command dispatch");
    assert!(controller.signals.is_empty());
    let ControlExecutionDetail::ReloadCommand { job_id, .. } = dispatch.execution.detail else {
        panic!("expected reload command job");
    };
    job_id
}

fn execute_and_launch_reload_command(supervisor: &mut Supervisor) -> crate::ids::JobId {
    let job_id = execute_reload_command(supervisor);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9100, 61)]);
    let mut clock = ScriptedClock::new([RELOAD_COMMAND_LAUNCH_NS]);
    supervisor
        .launch_next_pending_control_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch control job")
        .expect("reload command launch");
    job_id
}
