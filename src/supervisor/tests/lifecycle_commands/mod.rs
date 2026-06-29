mod active_already;
mod active_boundary;
mod control_execution;
mod inactive;
mod reload_command;
mod reset_clear;
mod stop_clear;

use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessLauncher, TestTokenProvider,
    alive_service, process, settings,
};

fn active_app_supervisor() -> Supervisor {
    let app = alive_service("app");
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

fn booted_supervisor(services: Vec<crate::service::ServiceDefinition>) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(services);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
}

fn completed_oneshot_supervisor() -> Supervisor {
    let mut task = super::oneshot_service("task");
    task.triggers.clear();
    task.remain_after_exit = true;
    let mut supervisor = booted_supervisor(vec![task]);
    let mut clock =
        ScriptedClock::new([super::LIFECYCLE_COMMAND_NS, super::LIFECYCLE_COMMAND_NS + 1]);
    supervisor
        .start_service("task", None, &mut clock)
        .expect("start task");
    launch_task(&mut supervisor, 8100, 51, &mut clock);
    let task_job = current_task_job(&supervisor);
    supervisor
        .complete_job(task_job, super::LIFECYCLE_COMMAND_NS + 2, 0)
        .expect("complete task");
    supervisor
}

fn failed_notify_supervisor() -> Supervisor {
    let mut task = crate::service::ServiceDefinition::simple_system_boot("task", "/sbin/task");
    task.triggers.clear();
    task.restart_policy = crate::service::RestartPolicy::Never;
    let mut supervisor = booted_supervisor(vec![task]);
    let mut clock =
        ScriptedClock::new([super::LIFECYCLE_COMMAND_NS, super::LIFECYCLE_COMMAND_NS + 1]);
    supervisor
        .start_service("task", None, &mut clock)
        .expect("start task");
    launch_task(&mut supervisor, 8200, 52, &mut clock);
    let task_job = current_task_job(&supervisor);
    supervisor
        .complete_job(task_job, super::LIFECYCLE_COMMAND_NS + 2, 1)
        .expect("fail task start");
    supervisor
}

fn abandoned_task_supervisor() -> Supervisor {
    let mut task = crate::service::ServiceDefinition::simple_system_boot("task", "/sbin/task");
    task.triggers.clear();
    let mut supervisor = booted_supervisor(vec![task]);
    supervisor
        .services
        .transition_service(
            "task",
            crate::service::runtime::ServiceTransition {
                to: crate::service::runtime::ServiceState::Starting,
                cause: crate::service::runtime::TransitionCause::ExplicitStart,
            },
        )
        .expect("starting task");
    supervisor
        .services
        .transition_service(
            "task",
            crate::service::runtime::ServiceTransition {
                to: crate::service::runtime::ServiceState::Stopping,
                cause: crate::service::runtime::TransitionCause::ExplicitStop,
            },
        )
        .expect("stopping task");
    supervisor
        .services
        .transition_service(
            "task",
            crate::service::runtime::ServiceTransition {
                to: crate::service::runtime::ServiceState::Abandoned,
                cause: crate::service::runtime::TransitionCause::ProcessUnkillable,
            },
        )
        .expect("abandoned task");
    supervisor
}

fn launch_task(supervisor: &mut Supervisor, pid: u32, pidfd: i32, clock: &mut ScriptedClock) {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, pidfd)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, clock)
        .expect("launch task")
        .expect("task launch dispatch");
}

fn current_task_job(supervisor: &Supervisor) -> crate::ids::JobId {
    supervisor
        .service_status("task")
        .expect("task status")
        .current_job
        .expect("task job")
        .id
}
