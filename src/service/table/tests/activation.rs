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

// PEI-353. The generation is "the tree at N is unusable, use N+1" — not a
// count of leaks. It used to increment once per leak *record*, and one failed
// start records two cleanup deadlines (`hooks` and `ServiceTree`) while a tree
// cleanup can report `main`, `hooks`, `health` and the root separately. All of
// those are the same tree, so N jumped by however many paths happened to be
// unreclaimable.
#[test]
fn every_leak_in_one_tree_advances_the_generation_once() {
    let mut table = table(&["app"]);

    for (path, kind) in [
        (
            "/sys/fs/cgroup/peinit/app/main",
            LeakedCgroupKind::ServiceTree,
        ),
        ("/sys/fs/cgroup/peinit/app/hooks", LeakedCgroupKind::Hooks),
        ("/sys/fs/cgroup/peinit/app/health", LeakedCgroupKind::Health),
        ("/sys/fs/cgroup/peinit/app", LeakedCgroupKind::ServiceTree),
    ] {
        assert!(
            table
                .record_leaked_cgroup("app", path.to_string(), kind, 1_000)
                .expect("record leak"),
            "{path} should be a new record",
        );
    }

    // Four records, all of the same tree, one generation.
    let runtime = table.runtime("app").expect("runtime");
    assert_eq!(runtime.leaked_cgroups.len(), 4);
    assert_eq!(runtime.cgroup_generation, 1);
}

// And a leak in the *new* tree does advance it again — the rule is per tree,
// not once ever.
#[test]
fn a_leak_in_the_current_tree_advances_the_generation_again() {
    let mut table = table(&["app"]);
    table
        .record_leaked_cgroup(
            "app",
            "/sys/fs/cgroup/peinit/app".to_string(),
            LeakedCgroupKind::ServiceTree,
            1_000,
        )
        .expect("first leak");
    assert_eq!(table.runtime("app").expect("runtime").cgroup_generation, 1,);

    table
        .record_leaked_cgroup(
            "app",
            "/sys/fs/cgroup/peinit/app%gen1/main".to_string(),
            LeakedCgroupKind::ServiceTree,
            2_000,
        )
        .expect("second leak");

    assert_eq!(table.runtime("app").expect("runtime").cgroup_generation, 2,);
}

/// PEI-596. A transition says which terminal it let go of, and it has to
/// say so even when the entry does not survive the transition.
///
/// The case is a first-boot setup flow on the way out: it removes its own
/// service definition so it never runs again, then exits. Both happen
/// within a second, so by the time anything reacts to the exit the entry
/// has been discarded and the table can no longer be asked what `TTYPath`
/// the service had. Read after the fact, the answer was "none", the
/// console was never handed on, and the machine sat on a finished setup
/// screen with no login prompt behind it.
#[test]
fn a_transition_reports_its_released_terminal_even_when_the_entry_goes() {
    let mut console = service("oobe-tui", "/bin/oobe-tui");
    console.console_path = Some("/dev/console".to_string());
    let mut table = ServiceTable::from_boot_snapshot(vec![console]).expect("service table");
    table
        .transition_service(
            "oobe-tui",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    table
        .transition_service(
            "oobe-tui",
            transition(ServiceState::Active, TransitionCause::ExplicitStart),
        )
        .expect("active");
    // Setup deletes its own key, then exits.
    table
        .apply_definition_snapshot(Vec::new())
        .expect("remove snapshot");

    let done = table
        .transition_service(
            "oobe-tui",
            transition(ServiceState::Inactive, TransitionCause::CleanExit),
        )
        .expect("exited");

    assert!(
        done.discarded_definition_removed,
        "a removed definition is discarded once nothing retains it",
    );
    assert!(
        table.get("oobe-tui").is_none(),
        "the entry is gone, which is why the terminal must ride on the transition",
    );
    assert_eq!(done.released_tty.as_deref(), Some("/dev/console"));
}

/// The other half: a transition that did not let go of a terminal says so
/// too, whether or not the service has one. Starting is holding, and a
/// service still holding must not have its console offered to a waiter.
#[test]
fn a_transition_that_keeps_or_never_had_a_terminal_releases_nothing() {
    let mut console = service("oobe-tui", "/bin/oobe-tui");
    console.console_path = Some("/dev/console".to_string());
    let mut table = ServiceTable::from_boot_snapshot(vec![console, service("app", "/sbin/app")])
        .expect("service table");

    let starting = table
        .transition_service(
            "oobe-tui",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    assert_eq!(starting.released_tty, None, "Inactive -> Starting takes it");

    let app = table
        .transition_service(
            "app",
            transition(ServiceState::Starting, TransitionCause::ExplicitStart),
        )
        .expect("starting");
    assert_eq!(
        app.released_tty, None,
        "a service with no TTYPath has none to give"
    );
}
