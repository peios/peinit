//! PEI-350: a boot executes against its snapshot (§3.7).
//!
//! The plan and the graph were always snapshotted; the registry was not. A
//! watch event during the boot ran a full reload, and a boot-plan service
//! that had not started yet started from a definition the plan never saw.
//! A reload during the boot window now runs, but leaves a boot-plan member
//! whose launch has not been attempted on the plan's definition with the
//! new one pending; the reload after the window applies it.

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
    assert_eq!(
        supervisor.frozen_boot_plan_members(),
        vec!["app".to_string()]
    );
    supervisor
}

/// Launch `app`; with Alive readiness that makes it Active, its start
/// operation terminal, and the boot window closed.
fn launch_app(supervisor: &mut Supervisor) {
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
    assert!(supervisor.frozen_boot_plan_members().is_empty());
}

fn changed_registry() -> StaticRegistry {
    let mut app = alive_service("app");
    app.image_path = "/sbin/app-v2".to_string();
    StaticRegistry::services(vec![app, alive_service("fresh")])
}

#[test]
fn a_reload_during_the_boot_window_runs_but_the_unlaunched_member_keeps_the_snapshot() {
    let mut supervisor = booting_with_app_pending();

    let outcome = supervisor
        .reload_config_from_registry(&mut changed_registry())
        .expect("the reload runs during the window");

    // Everything else applies as in any reload.
    assert_eq!(outcome.summary.added, vec!["fresh".to_string()]);
    assert!(supervisor.services().get("fresh").is_some());
    // The member whose launch has not been attempted is deferred: the
    // table keeps the plan's definition and records the new one as
    // pending, exactly as for a running service.
    assert_eq!(outcome.summary.deferred, vec!["app".to_string()]);
    assert!(outcome.summary.updated.is_empty());
    let app = supervisor.services().get("app").expect("app");
    assert_eq!(app.definition.image_path, "/sbin/app");
    assert_eq!(
        app.pending_definition
            .as_ref()
            .map(|d| d.image_path.as_str()),
        Some("/sbin/app-v2")
    );
    assert_eq!(app.runtime.state, ServiceState::Starting);
    assert!(supervisor.has_deferred_registry_reload());
    assert!(supervisor.take_due_deferred_registry_reload().is_none());
}

#[test]
fn the_boot_start_is_made_from_the_snapshot_and_the_change_lands_after_the_window() {
    let mut supervisor = booting_with_app_pending();
    supervisor
        .reload_config_from_registry(&mut changed_registry())
        .expect("reload during the window");
    let _ = supervisor
        .reload_config_from_registry(&mut changed_registry())
        .expect("a second reload during the window");

    launch_app(&mut supervisor);

    // The launch used the plan's definition.
    let job = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("app job");
    assert_eq!(
        supervisor
            .jobs()
            .get(job.id)
            .expect("job record")
            .image_path,
        "/sbin/app"
    );
    // One record for everything deferred during the window, taken once,
    // only once the window has closed.
    assert_eq!(
        supervisor.take_due_deferred_registry_reload(),
        Some(DeferredRegistryReload {
            services: vec!["app".to_string()],
        })
    );
    assert!(supervisor.take_due_deferred_registry_reload().is_none());
    assert!(!supervisor.has_deferred_registry_reload());
    // The reload after the window reaches the same end state a reload at
    // the time would have: the running service is pinned with the new
    // definition pending, and nothing is deferred any more.
    let outcome = supervisor
        .reload_config_from_registry(&mut changed_registry())
        .expect("the reload after the window");
    assert!(outcome.summary.deferred.is_empty());
    let app = supervisor.services().get("app").expect("app");
    assert_eq!(app.definition.image_path, "/sbin/app");
    assert_eq!(
        app.pending_definition
            .as_ref()
            .map(|d| d.image_path.as_str()),
        Some("/sbin/app-v2")
    );
    assert_eq!(app.runtime.state, ServiceState::Active);
}

#[test]
fn a_member_withdrawn_during_the_window_still_starts_and_is_removed_after_it() {
    let mut supervisor = booting_with_app_pending();

    let outcome = supervisor
        .reload_config_from_registry(&mut StaticRegistry::services(vec![alive_service("fresh")]))
        .expect("reload without app");

    assert_eq!(outcome.summary.deferred, vec!["app".to_string()]);
    assert!(outcome.summary.discarded.is_empty());
    assert!(outcome.summary.marked_removed.is_empty());
    let app = supervisor
        .services()
        .get("app")
        .expect("app is still in the table");
    assert!(!app.definition_removed);
    assert_eq!(app.definition.image_path, "/sbin/app");

    launch_app(&mut supervisor);
    assert!(supervisor.take_due_deferred_registry_reload().is_some());
    let outcome = supervisor
        .reload_config_from_registry(&mut StaticRegistry::services(vec![alive_service("fresh")]))
        .expect("the reload after the window");

    assert_eq!(outcome.summary.marked_removed, vec!["app".to_string()]);
    assert!(
        supervisor
            .services()
            .get("app")
            .expect("app")
            .definition_removed
    );
}

#[test]
fn a_member_whose_key_stops_decoding_during_the_window_still_starts_from_the_snapshot() {
    let mut supervisor = booting_with_app_pending();

    let outcome = supervisor
        .reload_config_from_registry(
            &mut StaticRegistry::services(Vec::new()).with_undecodable("app", "MissingImagePath"),
        )
        .expect("reload with app undecodable");

    assert_eq!(outcome.summary.deferred, vec!["app".to_string()]);
    assert!(outcome.summary.undecodable.is_empty());
    // The operator is still told which key and why.
    assert_eq!(outcome.undecodable.len(), 1);
    let app = supervisor.services().get("app").expect("app");
    assert_eq!(app.definition.image_path, "/sbin/app");
    assert!(!app.definition_removed);
    assert_eq!(app.runtime.state, ServiceState::Starting);

    launch_app(&mut supervisor);
}

/// A member whose launch has been attempted is not frozen: an Active one
/// pins with the new definition pending as any running service does, and
/// nothing is deferred.
#[test]
fn an_attempted_member_behaves_as_in_any_reload_during_the_window() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut app = alive_service("app");
    app.requires.push("authd".to_string());
    let mut registry = StaticRegistry::services(vec![app.clone(), alive_service("authd")]);
    supervisor
        .run_phase2_boot(&mut registry, &mut ScriptedClock::new([BOOT_NS]))
        .expect("boot");
    // authd launches (attempted); app is released by it but not yet launched.
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(
            &mut tokens,
            &mut launcher,
            &mut ScriptedClock::new([APP_LAUNCH_NS]),
        )
        .expect("launch authd")
        .expect("authd launch");
    assert_eq!(
        supervisor.service_status("authd").expect("authd").state,
        ServiceState::Active
    );
    assert!(supervisor.boot_plan_in_progress());
    assert_eq!(
        supervisor.frozen_boot_plan_members(),
        vec!["app".to_string()]
    );

    let mut authd = alive_service("authd");
    authd.image_path = "/sbin/authd-v2".to_string();
    let mut changed_app = app.clone();
    changed_app.image_path = "/sbin/app-v2".to_string();
    let outcome = supervisor
        .reload_config_from_registry(&mut StaticRegistry::services(vec![changed_app, authd]))
        .expect("reload during the window");

    // authd: attempted, so the ordinary running pin; app: frozen.
    assert_eq!(outcome.summary.updated, vec!["authd".to_string()]);
    assert_eq!(outcome.summary.deferred, vec!["app".to_string()]);
    let authd = supervisor.services().get("authd").expect("authd");
    assert_eq!(authd.definition.image_path, "/sbin/authd");
    assert_eq!(
        authd
            .pending_definition
            .as_ref()
            .map(|d| d.image_path.as_str()),
        Some("/sbin/authd-v2")
    );
}

#[test]
fn a_reload_that_changes_nothing_frozen_defers_nothing() {
    let mut supervisor = booting_with_app_pending();

    let outcome = supervisor
        .reload_config_from_registry(&mut StaticRegistry::services(vec![
            alive_service("app"),
            alive_service("fresh"),
        ]))
        .expect("reload during the window");

    assert!(outcome.summary.deferred.is_empty());
    assert_eq!(outcome.summary.added, vec!["fresh".to_string()]);
    assert!(!supervisor.has_deferred_registry_reload());
    launch_app(&mut supervisor);
    assert!(supervisor.take_due_deferred_registry_reload().is_none());
}

#[test]
fn a_boot_plan_with_nothing_to_start_has_no_window() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut app = alive_service("app");
    app.triggers.clear();
    let mut registry = StaticRegistry::services(vec![app]);
    supervisor
        .run_phase2_boot(&mut registry, &mut ScriptedClock::new([BOOT_NS]))
        .expect("boot");

    assert!(!supervisor.boot_plan_in_progress());
    assert!(supervisor.frozen_boot_plan_members().is_empty());
    let outcome = supervisor
        .reload_config_from_registry(&mut changed_registry())
        .expect("reload");
    assert_eq!(outcome.summary.updated, vec!["app".to_string()]);
    assert!(outcome.summary.deferred.is_empty());
}

#[test]
fn a_supervisor_that_never_booted_has_no_boot_window() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));

    assert!(!supervisor.boot_plan_in_progress());
    assert!(supervisor.frozen_boot_plan_members().is_empty());
    assert!(supervisor.take_due_deferred_registry_reload().is_none());
}
