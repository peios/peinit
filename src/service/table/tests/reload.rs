use crate::service::ServiceSecurityDescriptor;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::{service, table, transition};

#[test]
fn reload_snapshot_adds_updates_and_discards_inactive_removed_services() {
    let mut table = table(&["old", "changed"]);
    let mut changed = service("changed", "/sbin/changed-v2");
    changed.arguments.push("--new".to_string());

    let summary = table
        .apply_definition_snapshot(vec![changed, service("added", "/sbin/added")])
        .expect("reload");

    assert_eq!(summary.added, vec!["added"]);
    assert_eq!(summary.updated, vec!["changed"]);
    assert!(summary.restored.is_empty());
    assert!(summary.marked_removed.is_empty());
    assert_eq!(summary.discarded, vec!["old"]);
    assert!(table.get("old").is_none());
    assert_eq!(
        table.definition("changed").expect("changed").image_path,
        "/sbin/changed-v2",
    );
    assert_eq!(
        table.runtime("added").expect("added runtime").state,
        ServiceState::Inactive,
    );
}

#[test]
fn running_reload_preserves_activation_fields_and_records_pending_definition() {
    let mut table = table(&["app"]);
    table
        .transition_service(
            "app",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    table
        .transition_service(
            "app",
            transition(ServiceState::Active, TransitionCause::ExplicitStart),
        )
        .expect("active");

    let mut changed = service("app", "/sbin/app-v2");
    changed.identity = "LocalService".to_string();
    changed.required_privileges = vec!["SeChangeNotifyPrivilege".to_string()];
    changed.requires = vec!["db".to_string()];
    changed.start_timeout_secs = 99;
    changed.display_name = Some("App v2".to_string());
    changed.service_security = ServiceSecurityDescriptor::RegistryBinary(vec![1, 2, 3]);

    let summary = table
        .apply_definition_snapshot(vec![changed.clone(), service("db", "/sbin/db")])
        .expect("reload");

    assert_eq!(summary.updated, vec!["app"]);
    let entry = table.get("app").expect("app");
    assert_eq!(entry.definition.image_path, "/sbin/app");
    assert_eq!(entry.definition.identity, "SYSTEM");
    assert!(entry.definition.required_privileges.is_empty());
    assert!(entry.definition.requires.is_empty());
    assert_eq!(entry.definition.start_timeout_secs, 99);
    assert_eq!(entry.definition.display_name.as_deref(), Some("App v2"));
    assert_eq!(
        entry.definition.service_security,
        ServiceSecurityDescriptor::RegistryBinary(vec![1, 2, 3]),
    );
    assert_eq!(entry.pending_definition.as_ref(), Some(&changed));
}

#[test]
fn pending_definition_is_promoted_after_running_activation_drains() {
    let mut table = table(&["app"]);
    table
        .transition_service(
            "app",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    table
        .transition_service(
            "app",
            transition(ServiceState::Active, TransitionCause::ExplicitStart),
        )
        .expect("active");
    let mut changed = service("app", "/sbin/app-v2");
    changed.requires = vec!["db".to_string()];
    table
        .apply_definition_snapshot(vec![changed.clone(), service("db", "/sbin/db")])
        .expect("reload");

    table
        .transition_service(
            "app",
            transition(ServiceState::Failed, TransitionCause::ProcessCrash),
        )
        .expect("failed");

    let entry = table.get("app").expect("app");
    assert_eq!(entry.definition, changed);
    assert!(entry.pending_definition.is_none());
}

#[test]
fn reload_snapshot_marks_running_removed_services_without_killing_them() {
    let mut table = table(&["app"]);
    table
        .transition_service(
            "app",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    table
        .transition_service(
            "app",
            transition(ServiceState::Active, TransitionCause::ExplicitStart),
        )
        .expect("active");

    let summary = table
        .apply_definition_snapshot(Vec::new())
        .expect("remove snapshot");

    assert!(summary.added.is_empty());
    assert_eq!(summary.marked_removed, vec!["app"]);
    assert!(summary.discarded.is_empty());
    let entry = table.get("app").expect("retained removed service");
    assert!(entry.definition_removed);
    assert_eq!(entry.runtime.state, ServiceState::Active);
}

#[test]
fn removed_running_service_is_discarded_after_it_drains() {
    let mut table = table(&["app"]);
    table
        .transition_service(
            "app",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    table
        .transition_service(
            "app",
            transition(ServiceState::Active, TransitionCause::ExplicitStart),
        )
        .expect("active");
    table
        .apply_definition_snapshot(Vec::new())
        .expect("remove snapshot");

    let stopping = table
        .transition_service(
            "app",
            transition(ServiceState::Stopping, TransitionCause::ExplicitStop),
        )
        .expect("stopping");
    assert!(!stopping.discarded_definition_removed);
    assert!(table.get("app").is_some());

    let inactive = table
        .transition_service(
            "app",
            transition(ServiceState::Inactive, TransitionCause::ExplicitStop),
        )
        .expect("inactive");
    assert!(inactive.discarded_definition_removed);
    assert!(table.get("app").is_none());
}

#[test]
fn reappearing_definition_restores_removed_entry() {
    let mut table = table(&["app"]);
    table
        .transition_service(
            "app",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    table
        .transition_service(
            "app",
            transition(ServiceState::Active, TransitionCause::ExplicitStart),
        )
        .expect("active");
    table
        .apply_definition_snapshot(Vec::new())
        .expect("remove snapshot");

    let summary = table
        .apply_definition_snapshot(vec![service("app", "/sbin/app-v2")])
        .expect("restore snapshot");

    assert!(summary.added.is_empty());
    assert_eq!(summary.updated, vec!["app"]);
    assert_eq!(summary.restored, vec!["app"]);
    let entry = table.get("app").expect("restored");
    assert!(!entry.definition_removed);
    assert_eq!(entry.definition.image_path, "/sbin/app");
    assert_eq!(
        entry
            .pending_definition
            .as_ref()
            .expect("pending definition")
            .image_path,
        "/sbin/app-v2",
    );
    assert_eq!(entry.runtime.state, ServiceState::Active);
}
