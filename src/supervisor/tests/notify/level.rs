use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::service::ServiceDefinition;
use crate::service::runtime::ServiceState;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::super::{
    ADMIN_START_NS, AUTHD_LAUNCH_NS, BOOT_NS, ON_DEMAND_APP_LAUNCH_NS, ScriptedClock,
    StaticRegistry, TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};
use super::{NOTIFY_NS, apply_notify, datagram};

/// The PEI-500 bug, end to end: `Requires = ["authd:sessions"]` against an
/// authd that is already active. The target is not in the start plan, so
/// before level edges the context had nothing to wait on and the dependent
/// started unconditionally.
#[test]
fn a_level_requirement_on_an_active_service_holds_until_the_level_arrives() {
    let mut app = alive_service("app");
    app.triggers.clear();
    app.requires.push("authd:sessions".to_string());
    let authd = ServiceDefinition::simple_system_boot("authd", "/sbin/authd");

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, authd]);
    let mut clock = ScriptedClock::new([
        BOOT_NS,
        AUTHD_LAUNCH_NS,
        ADMIN_START_NS,
        ON_DEMAND_APP_LAUNCH_NS,
    ]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch authd")
        .expect("authd launch dispatch");
    apply_notify(&mut supervisor, datagram(4242, b"READY=1"), NOTIFY_NS)
        .expect("authd becomes active");
    assert_eq!(
        supervisor.service_status("authd").expect("authd").state,
        ServiceState::Active,
    );

    let start = supervisor
        .start_service("app", None, &mut clock)
        .expect("start app");
    let LifecycleCommandOutcome::OnDemandStart(_) = &start.outcome else {
        panic!("expected on-demand start");
    };
    assert!(
        start.start_dispatches.is_empty(),
        "app must be held: authd has not published the level"
    );
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );

    let wrong = apply_notify(
        &mut supervisor,
        datagram(4242, b"LEVEL=maintenance\nLEVEL=sessions,maintenance"),
        ADMIN_START_NS + 1_000,
    )
    .expect("apply wrong level");
    assert!(
        wrong.start_dispatches.is_empty(),
        "a different level must not open the gate: exact match only"
    );

    let released = apply_notify(
        &mut supervisor,
        datagram(
            4242,
            b"LEVEL=maintenance\nLEVEL=sessions\nLEVEL=maintenance\nLEVEL=",
        ),
        ADMIN_START_NS + 2_000,
    )
    .expect("apply wanted level");
    assert_eq!(
        released
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    let mut launcher = TestProcessLauncher::new(vec![process(4300, 10)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    let levels =
        |supervisor: &Supervisor| supervisor.services.runtime("authd").unwrap().levels.clone();
    assert_eq!(
        levels(&supervisor),
        ["maintenance".to_string(), "sessions".to_string()].into()
    );
    apply_notify(
        &mut supervisor,
        datagram(4242, b"STATUS=still serving"),
        NOTIFY_NS + 3,
    )
    .expect("unrelated notify");
    assert_eq!(
        levels(&supervisor).len(),
        2,
        "no LEVEL leaves the set intact"
    );
    apply_notify(
        &mut supervisor,
        datagram(4242, b"LEVEL=maintenance"),
        NOTIFY_NS + 4,
    )
    .expect("replace set");
    assert_eq!(levels(&supervisor), ["maintenance".to_string()].into());
    assert_eq!(
        supervisor.service_status("app").unwrap().state,
        ServiceState::Active,
        "retracting a level does not stop an already-started dependent"
    );
    apply_notify(&mut supervisor, datagram(4242, b"LEVEL="), NOTIFY_NS + 5).expect("clear set");
    assert!(levels(&supervisor).is_empty());
}

/// At boot the target *is* a member, and it reaches Active (READY=1) well
/// before it publishes its level — netd is up long before DHCP finishes.
/// The member edge settling must not release the dependent on its own.
#[test]
fn a_boot_dependent_waits_past_ready_for_the_level() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("authd:sessions".to_string());
    let mut authd = ServiceDefinition::simple_system_boot("authd", "/sbin/authd");
    authd.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, authd]);
    let mut clock = ScriptedClock::new([BOOT_NS, AUTHD_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch authd")
        .expect("authd launch dispatch");

    let ready = apply_notify(&mut supervisor, datagram(4242, b"READY=1"), NOTIFY_NS)
        .expect("authd becomes active");
    assert_eq!(
        supervisor.service_status("authd").expect("authd").state,
        ServiceState::Active,
    );
    assert!(
        ready.start_dispatches.is_empty(),
        "READY alone must not release a level dependent"
    );

    let released = apply_notify(
        &mut supervisor,
        datagram(4242, b"LEVEL=sessions"),
        NOTIFY_NS + 1,
    )
    .expect("apply wanted level");
    assert_eq!(
        released
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
}

#[test]
fn ready_and_all_levels_in_one_datagram_release_a_boot_dependent_in_any_field_order() {
    for payload in [
        b"LEVEL=sessions\nREADY=1\nLEVEL=maintenance".as_slice(),
        b"READY=1\nLEVEL=maintenance\nLEVEL=sessions".as_slice(),
    ] {
        let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
        app.requires.push("authd:sessions".to_string());
        let mut authd = ServiceDefinition::simple_system_boot("authd", "/sbin/authd");
        authd.triggers.clear();
        let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
        let mut registry = StaticRegistry::services(vec![app, authd]);
        let mut clock = ScriptedClock::new([BOOT_NS, AUTHD_LAUNCH_NS]);
        supervisor
            .run_phase2_boot(&mut registry, &mut clock)
            .unwrap();
        let mut tokens = TestTokenProvider::default();
        let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
        supervisor
            .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
            .unwrap()
            .expect("authd launch");
        let released = apply_notify(&mut supervisor, datagram(4242, payload), NOTIFY_NS).unwrap();
        assert_eq!(
            released
                .start_dispatches
                .iter()
                .map(|d| d.ready.service.as_str())
                .collect::<Vec<_>>(),
            ["app"]
        );
        assert_eq!(
            supervisor.services.runtime("authd").unwrap().levels.len(),
            2
        );
    }
}
