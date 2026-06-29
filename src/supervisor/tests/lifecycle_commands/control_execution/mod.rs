mod reload;
mod restart;
mod stop;

use crate::control::lifecycle::LifecycleCommandOutcome;

use super::active_app_supervisor;
use crate::supervisor::tests::{
    LIFECYCLE_COMMAND_NS, RESTART_LAUNCH_NS, ScriptedClock, TestProcessLauncher, TestTokenProvider,
    process,
};

const CONTROL_NS: u64 = LIFECYCLE_COMMAND_NS + 1;
const EXIT_NS: u64 = LIFECYCLE_COMMAND_NS + 2;

fn stop_app(supervisor: &mut crate::supervisor::Supervisor) -> crate::ids::OperationId {
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let accepted = supervisor
        .stop_service("app", None, &mut clock)
        .expect("stop app");
    let LifecycleCommandOutcome::OperationAccepted(operation) = accepted.outcome else {
        panic!("expected stop operation");
    };
    operation.returned_operation_id
}

fn current_app_job(supervisor: &crate::supervisor::Supervisor) -> crate::ids::JobId {
    supervisor
        .service_status("app")
        .expect("app status")
        .current_job
        .expect("app job")
        .id
}
