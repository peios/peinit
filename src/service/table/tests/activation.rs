use crate::service::runtime::{LeakedCgroupKind, ServiceState, TransitionCause};
use crate::service::{RestartBackoffDeadline, ServiceTable, ServiceTableError};

use super::{service, table, transition};

#[test]
fn boot_snapshot_creates_inactive_entries_and_rejects_duplicates() {
    let table = table(&["authd", "registryd"]);

    assert_eq!(table.service_names(), vec!["authd", "registryd"]);
    assert_eq!(
        table.runtime("authd").expect("runtime").state,
        ServiceState::Inactive,
    );
    assert!(!table.get("registryd").expect("entry").definition_removed);

    let err = ServiceTable::from_boot_snapshot(vec![
        service("dupe", "/sbin/one"),
        service("dupe", "/sbin/two"),
    ])
    .expect_err("duplicate");
    assert_eq!(
        err,
        ServiceTableError::DuplicateService {
            service: "dupe".to_string(),
        }
    );
}

#[test]
fn activation_snapshot_clones_current_definition_and_next_generation() {
    let mut table = table(&["app"]);

    let snapshot = table
        .prepare_activation_snapshot("app")
        .expect("activation snapshot");
    assert_eq!(snapshot.service, "app");
    assert_eq!(snapshot.activation_generation, 1);
    assert_eq!(snapshot.cgroup_generation, 0);
    assert_eq!(snapshot.definition.image_path, "/sbin/app");

    table
        .transition_service(
            "app",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    let snapshot = table
        .prepare_activation_snapshot("app")
        .expect("next activation snapshot");
    assert_eq!(snapshot.activation_generation, 2);
    assert_eq!(snapshot.cgroup_generation, 0);
}

#[test]
fn unknown_and_definition_removed_services_cannot_prepare_activation_snapshot() {
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

    assert_eq!(
        table
            .prepare_activation_snapshot("missing")
            .expect_err("unknown"),
        ServiceTableError::UnknownService {
            service: "missing".to_string(),
        }
    );
    assert_eq!(
        table
            .prepare_activation_snapshot("app")
            .expect_err("definition removed"),
        ServiceTableError::DefinitionRemoved {
            service: "app".to_string(),
        }
    );
}

#[test]
fn leaked_cgroup_advances_cgroup_generation_only() {
    for (path, kind) in [
        ("/sys/fs/cgroup/peinit/app/health", LeakedCgroupKind::Health),
        ("/sys/fs/cgroup/peinit/app/hooks", LeakedCgroupKind::Hooks),
        ("/sys/fs/cgroup/peinit/app", LeakedCgroupKind::ServiceTree),
    ] {
        let mut table = table(&["app"]);
        table
            .record_leaked_cgroup("app", path.to_string(), kind, 1_000)
            .expect("record leak");

        let runtime = table.runtime("app").expect("runtime");
        assert_eq!(runtime.generation, 0);
        assert_eq!(runtime.cgroup_generation, 1);

        let snapshot = table
            .prepare_activation_snapshot("app")
            .expect("activation snapshot");
        assert_eq!(snapshot.activation_generation, 1);
        assert_eq!(snapshot.cgroup_generation, 1);
    }
}

#[test]
fn restart_backoff_transition_records_pending_restart_metadata() {
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
        .transition_service_to_restart_backoff("app", TransitionCause::ProcessCrash, 2_000)
        .expect("backoff");

    let runtime = table.runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Backoff);
    assert_eq!(runtime.consecutive_restart_failures, 1);
    assert_eq!(runtime.restart_backoff_until_ns, Some(2_000));
}

#[test]
fn restart_backoff_deadlines_report_due_services_in_name_order() {
    let mut table = table(&["web", "cache", "worker"]);
    put_active_service_in_backoff(&mut table, "worker", 3_000);
    put_active_service_in_backoff(&mut table, "web", 1_000);
    put_active_service_in_backoff(&mut table, "cache", 1_000);

    assert_eq!(
        table.due_restart_backoffs(1_500),
        vec![
            RestartBackoffDeadline {
                service: "cache".to_string(),
                due_at_ns: 1_000,
            },
            RestartBackoffDeadline {
                service: "web".to_string(),
                due_at_ns: 1_000,
            },
        ]
    );
    assert_eq!(
        table.next_restart_backoff_deadline(),
        Some(RestartBackoffDeadline {
            service: "cache".to_string(),
            due_at_ns: 1_000,
        })
    );
}

#[test]
fn restart_backoff_deadlines_skip_removed_definitions() {
    let mut table = table(&["app"]);
    put_active_service_in_backoff(&mut table, "app", 1_000);
    table
        .apply_definition_snapshot(Vec::new())
        .expect("remove snapshot");

    assert!(table.due_restart_backoffs(2_000).is_empty());
    assert_eq!(table.next_restart_backoff_deadline(), None);
}

fn put_active_service_in_backoff(table: &mut ServiceTable, service: &str, due_at_ns: u64) {
    table
        .transition_service(
            service,
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    table
        .transition_service(
            service,
            transition(ServiceState::Active, TransitionCause::ExplicitStart),
        )
        .expect("active");
    table
        .transition_service_to_restart_backoff(service, TransitionCause::ProcessCrash, due_at_ns)
        .expect("backoff");
}
