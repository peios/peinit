//! PEI-350: a boot executes against its snapshot (§3.7).
//!
//! The plan and the graph were always snapshotted; the registry was not. A
//! watch event during the boot ran a full reload, and a boot-plan service
//! that had not started yet started from a definition the plan never saw.
//! The reload is now gated on the boot plan having drained, and everything
//! that arrived during the boot is coalesced into one reload afterwards.

use crate::control::reload_config::ReloadConfigError;
use crate::service::ServiceDefinition;
use crate::service::runtime::ServiceState;
use crate::supervisor::{DeferredRegistryReload, Supervisor, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessLauncher, TestTokenProvider,
    alive_service, process, settings,
};

/// A boot whose plan has one service, `app`, Starting and waiting for its
/// launch — the window in which a reload used to replace its definition.
fn booting_with_app_pending() -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![alive_service("app")]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting
    );
    assert!(supervisor.boot_plan_in_progress());
    supervisor
}

/// Launch `app`; with Alive readiness that makes it Active, its start
/// operation terminal, and the boot plan drained.
fn drain_the_plan(supervisor: &mut Supervisor) {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active
    );
    assert!(!supervisor.boot_plan_in_progress());
}

fn changed_registry() -> StaticRegistry {
    let mut app = alive_service("app");
    app.image_path = "/sbin/app-v2".to_string();
    StaticRegistry::services(vec![app, alive_service("fresh")])
}

#[test]
fn a_reload_during_the_boot_window_is_refused_and_the_snapshot_holds() {
    let mut supervisor = booting_with_app_pending();
    let before = supervisor.services().clone();

    let refused = supervisor
        .reload_config_from_registry(&mut changed_registry())
        .expect_err("the boot executes against its snapshot");

    assert_eq!(refused, ReloadConfigError::BootInProgress);
    // Nothing moved: not the not-yet-started service's definition, not the
    // table's membership.
    assert_eq!(supervisor.services(), &before);
    assert_eq!(
        supervisor
            .services()
            .definition("app")
            .expect("app")
            .image_path,
        ServiceDefinition::simple_system_boot("app", "/sbin/app").image_path
    );
    assert!(supervisor.services().get("fresh").is_none());
    // The request is remembered, but not due while the plan is draining.
    assert!(supervisor.has_deferred_registry_reload());
    assert!(supervisor.take_due_deferred_registry_reload().is_none());
}

#[test]
fn everything_deferred_during_the_boot_window_is_one_reload_after_the_plan_drains() {
    let mut supervisor = booting_with_app_pending();
    supervisor.defer_registry_watch_reload(91, 2, false);
    supervisor.defer_registry_watch_reload(92, 3, true);
    let _ = supervisor.reload_config_from_registry(&mut changed_registry());
    assert!(supervisor.take_due_deferred_registry_reload().is_none());

    drain_the_plan(&mut supervisor);

    // One record for the lot, taken once.
    assert_eq!(
        supervisor.take_due_deferred_registry_reload(),
        Some(DeferredRegistryReload {
            watch_fd: Some(92),
            watch_events: 5,
            overflow: true,
            explicit_requests: 1,
        })
    );
    assert!(supervisor.take_due_deferred_registry_reload().is_none());
    assert!(!supervisor.has_deferred_registry_reload());

    // And the reload it stands for reaches the same end state a reload at
    // the time would have: the service that started from the snapshot has
    // the new definition pending, and the new service exists.
    let outcome = supervisor
        .reload_config_from_registry(&mut changed_registry())
        .expect("the reload runs once the plan has drained");
    assert_eq!(outcome.summary.added, vec!["fresh".to_string()]);
    assert_eq!(outcome.summary.updated, vec!["app".to_string()]);
    let app = supervisor.services().get("app").expect("app");
    assert_eq!(app.definition.image_path, "/sbin/app");
    assert_eq!(
        app.pending_definition
            .as_ref()
            .expect("pending definition")
            .image_path,
        "/sbin/app-v2"
    );
    assert!(supervisor.services().get("fresh").is_some());
}

#[test]
fn a_boot_plan_with_nothing_to_start_is_drained_at_once() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut app = alive_service("app");
    app.triggers.clear();
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot");

    assert!(!supervisor.boot_plan_in_progress());
    assert!(
        supervisor
            .reload_config_from_registry(&mut changed_registry())
            .is_ok()
    );
}

#[test]
fn a_supervisor_that_never_booted_has_no_boot_window() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));

    assert!(!supervisor.boot_plan_in_progress());
    assert!(supervisor.take_due_deferred_registry_reload().is_none());
}
