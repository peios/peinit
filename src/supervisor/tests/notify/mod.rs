mod fd_store;
mod readiness;
mod reload;
mod shutdown;
mod timeout_extension;

use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::service::ServiceDefinition;
use crate::supervisor::{
    Supervisor, SupervisorError, SupervisorNotifyDispatch, SupervisorSettings,
};

use super::{
    APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, alive_service, process,
    settings,
};

const NOTIFY_NS: u64 = super::AUTHD_LAUNCH_NS + 1_000;
const CONTROL_NS: u64 = LIFECYCLE_COMMAND_NS + 1;
const RELOAD_NOTIFY_NS: u64 = CONTROL_NS + 1_000;

fn notify_app_supervisor() -> Supervisor {
    let app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot app");
    launch_app(&mut supervisor, &mut clock, 8000, 50);
    supervisor
}

fn active_app_supervisor() -> Supervisor {
    let app = alive_service("app");
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot app");
    launch_app(&mut supervisor, &mut clock, 8000, 50);
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

fn launch_app(supervisor: &mut Supervisor, clock: &mut ScriptedClock, pid: u32, pidfd: i32) {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, pidfd)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, clock)
        .expect("launch app")
        .expect("app launch dispatch");
}

fn datagram(pid: u32, payload: &[u8]) -> NotifyDatagram {
    NotifyDatagram {
        payload: payload.to_vec(),
        credentials: NotifyCredentials {
            pid,
            uid: 0,
            gid: 0,
        },
        fds: Vec::new(),
    }
}

fn apply_notify(
    supervisor: &mut Supervisor,
    datagram: NotifyDatagram,
    observed_at_ns: u64,
) -> Result<SupervisorNotifyDispatch, SupervisorError> {
    let mut controller = TestProcessController::default();
    apply_notify_with_controller(supervisor, datagram, observed_at_ns, &mut controller)
}

fn apply_notify_with_controller(
    supervisor: &mut Supervisor,
    datagram: NotifyDatagram,
    observed_at_ns: u64,
    controller: &mut TestProcessController,
) -> Result<SupervisorNotifyDispatch, SupervisorError> {
    supervisor
        .apply_notify_datagram(datagram, observed_at_ns, controller)
        .map(|outcome| outcome.into_service().expect("service notify outcome"))
}
