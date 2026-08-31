use crate::service::ServiceTrigger;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::supervisor::{Supervisor, SupervisorSettings, SupervisorTimerAction};

use super::{
    BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, oneshot_service, process, settings,
};

#[test]
fn timer_firing_for_inactive_simple_creates_timer_start() {
    let mut app = alive_service("app");
    app.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let dispatch = supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS)
        .expect("timer dispatch");

    let SupervisorTimerAction::Start {
        outcome,
        start_dispatches,
        ..
    } = dispatch.action
    else {
        panic!("expected timer start");
    };
    assert_eq!(
        outcome.plan.requested_operation_source,
        crate::operation::OperationSource::Timer,
    );
    assert_eq!(
        outcome.plan.requested_transition_cause,
        TransitionCause::Timer,
    );
    assert_eq!(
        start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    assert_eq!(
        supervisor.services().runtime("app").expect("runtime").state,
        ServiceState::Starting,
    );
}

#[test]
fn timer_firing_for_running_oneshot_sets_one_pending_flag() {
    let mut app = oneshot_service("app");
    app.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
        .services
        .transition_service(
            "app",
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::Timer,
            },
        )
        .expect("starting");

    let first = supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS)
        .expect("first timer dispatch");
    assert_eq!(
        first.action,
        SupervisorTimerAction::PendingOneshot {
            newly_pending: true,
        },
    );

    let second = supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS + 1)
        .expect("second timer dispatch");
    assert_eq!(
        second.action,
        SupervisorTimerAction::PendingOneshot {
            newly_pending: false,
        },
    );
    assert!(
        supervisor
            .services()
            .runtime("app")
            .expect("runtime")
            .pending_timer,
    );
}

#[test]
fn timer_firing_for_active_simple_is_noop() {
    let mut app = alive_service("app");
    app.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
        .services
        .transition_service(
            "app",
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::Timer,
            },
        )
        .expect("starting");
    supervisor
        .services
        .transition_service(
            "app",
            ServiceTransition {
                to: ServiceState::Active,
                cause: TransitionCause::Timer,
            },
        )
        .expect("active");

    let dispatch = supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS)
        .expect("timer dispatch");

    assert_eq!(dispatch.action, SupervisorTimerAction::SimpleNoop);
}

#[test]
fn timer_firing_for_disabled_service_is_noop() {
    let mut app = alive_service("app");
    app.disabled = true;
    app.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let dispatch = supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS)
        .expect("timer dispatch");

    assert_eq!(dispatch.action, SupervisorTimerAction::Disabled);
    assert_eq!(
        supervisor.services().runtime("app").expect("runtime").state,
        ServiceState::Inactive,
    );
}

#[test]
fn timer_firing_for_skipped_service_is_state_noop() {
    let mut app = alive_service("app");
    app.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
        .services
        .transition_service(
            "app",
            ServiceTransition {
                to: ServiceState::Skipped,
                cause: TransitionCause::ConditionSkipped,
            },
        )
        .expect("skipped");

    let dispatch = supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS)
        .expect("timer dispatch");

    assert_eq!(
        dispatch.action,
        SupervisorTimerAction::StateNoop {
            state: ServiceState::Skipped,
        },
    );
}

#[test]
fn pending_oneshot_timer_run_starts_when_current_run_completes() {
    let mut app = oneshot_service("app");
    app.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, LIFECYCLE_COMMAND_NS + 10_000]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS)
        .expect("initial timer dispatch");
    supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS + 1)
        .expect("pending timer dispatch");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    let launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch")
        .expect("launch dispatch");

    let terminal = supervisor
        .complete_job(
            launch.started.job_event.job_id,
            LIFECYCLE_COMMAND_NS + 20_000,
            0,
        )
        .expect("terminal");

    assert_eq!(
        terminal
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    let runtime = supervisor.services().runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Starting);
    assert!(!runtime.pending_timer);
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
}

// PEI-339. A shutdown disarms timer triggers: "While the flag is set: no new
// services may be started; timer triggers are disarmed."
//
// This one mattered more than the rule reads. The states classify_timer_firing
// starts from — an idle Oneshot in Inactive, Completed or Failed — are exactly
// the states shutdown classifies as not participating, so a firing would start
// a brand-new service *after* the stop waves were frozen. Nothing then stops
// it: it is in no wave, wave_complete never accounts for it, and
// remaining_shutdown_services walks only plan.stop_waves, so even the 90-second
// global timeout sweep does not reach it. It survives to the unmount step
// holding files open on filesystems peinit is trying to unmount.
#[test]
fn a_timer_firing_during_shutdown_starts_nothing() {
    let mut app = oneshot_service("app");
    app.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(
            crate::shutdown::ShutdownKind::Poweroff,
            &mut controller,
            LIFECYCLE_COMMAND_NS,
        )
        .expect("begin shutdown");

    let dispatch = supervisor
        .handle_timer_firing("app", "daily UTC", LIFECYCLE_COMMAND_NS + 1)
        .expect("a timer firing during shutdown is not an error");

    assert_eq!(dispatch.action, SupervisorTimerAction::ShutdownInProgress);
    // Not started, and not queued to start later either: a pending-timer mark
    // would fire the run at the next terminal event, which is a thing shutdown
    // produces in quantity.
    let runtime = supervisor.services().runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Inactive);
    assert!(!runtime.pending_timer);
}
