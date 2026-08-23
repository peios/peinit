use crate::boundary::{
    BoundaryError, FilesystemCheckHelperLauncher, FilesystemCheckHelperRequest,
    FilesystemCheckReport, FilesystemCheckResult, LaunchedFilesystemCheckHelper,
};
use crate::operation::OperationState;
use crate::service::runtime::{LeakedCgroupKind, ServiceState, TransitionCause};
use crate::service::{ServiceCheck, ServiceCheckKind};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};

#[test]
fn skipped_required_dependency_releases_dependent_start() {
    let mut app = alive_service("app");
    app.requires.push("db".to_string());
    let mut db = alive_service("db");
    db.conditions = vec![registry_check("Machine\\System\\Services\\missing")];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, db]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);

    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    assert_eq!(
        boot.start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Skipped,
    );
    assert_eq!(
        supervisor.service_status("db").expect("db").cause,
        Some(TransitionCause::ConditionSkipped),
    );
    let db_operation = operation_for_service(&boot.plan, "db");
    assert_eq!(
        supervisor
            .operation_status(db_operation)
            .expect("db operation")
            .state,
        OperationState::Completed,
    );

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(7200, 72)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
}

#[test]
fn asserted_required_dependency_failure_fails_dependent_start() {
    let mut app = alive_service("app");
    app.requires.push("db".to_string());
    let mut db = alive_service("db");
    db.asserts = vec![registry_check("Machine\\System\\Services\\missing")];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, db]);
    let mut clock = ScriptedClock::new([BOOT_NS]);

    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    assert!(boot.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Failed,
    );
    assert_eq!(
        supervisor.service_status("db").expect("db").cause,
        Some(TransitionCause::AssertionError),
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Failed,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").cause,
        Some(TransitionCause::DependencyFailure),
    );
    for service in ["db", "app"] {
        assert_eq!(
            supervisor
                .operation_status(operation_for_service(&boot.plan, service))
                .expect("operation")
                .state,
            OperationState::Failed,
        );
    }
}

#[test]
fn skipped_root_condition_prunes_unresolved_dependency() {
    let mut app = alive_service("app");
    app.requires.push("db".to_string());
    app.conditions = vec![registry_check("Machine\\System\\Services\\missing")];
    let mut db = alive_service("db");
    db.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, db]);
    let mut clock = ScriptedClock::new([BOOT_NS]);

    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let app_operation = operation_for_service(&boot.plan, "app");
    let db_operation = operation_for_service(&boot.plan, "db");
    assert!(boot.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Skipped,
    );
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Inactive,
    );
    assert_eq!(
        supervisor
            .operation_status(app_operation)
            .expect("app operation")
            .state,
        OperationState::Completed,
    );
    assert_eq!(
        supervisor
            .operation_status(db_operation)
            .expect("db operation")
            .state,
        OperationState::Cancelled,
    );
}

#[test]
fn filesystem_condition_queues_and_launches_helper_without_service_job() {
    let mut app = alive_service("app");
    app.conditions = vec![ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/app".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);

    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let operation_id = operation_for_service(&boot.plan, "app");
    assert!(boot.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(
        supervisor.pending_pre_start_check_launches(),
        vec![operation_id]
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );

    let mut launcher = TestFilesystemCheckLauncher::default();
    let launch = supervisor
        .launch_next_pending_filesystem_check_helper(&mut launcher, BOOT_NS + 1)
        .expect("launch helper")
        .expect("helper dispatch");

    assert_eq!(launch.helper.operation_id, operation_id);
    assert_eq!(launch.helper.result_fd, 81);
    assert_eq!(supervisor.pending_pre_start_check_launches(), Vec::new());
    assert_eq!(launcher.requests.len(), 1);
    assert_eq!(
        launcher.requests[0].cgroup_id,
        "/sys/fs/cgroup/peinit/app/checks",
    );
    assert_eq!(launcher.requests[0].checks, launch.helper.checks);
}

/// The D-state path for a pre-start check helper.
///
/// A helper whose cgroup is still populated after SIGKILL cannot be removed,
/// so it is abandoned. It used to be abandoned *silently* -- the cleanup
/// matched `CgroupCleanupKind::Helper => Ok(())` and dropped it, so there was
/// no record anywhere and, worse, no generation bump, leaving the next start
/// to reuse a tree containing an unkillable process.
#[test]
fn a_populated_filesystem_check_helper_cgroup_is_recorded_as_leaked() {
    let mut app = alive_service("app");
    app.conditions = vec![ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/app".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);

    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let operation_id = operation_for_service(&boot.plan, "app");
    let mut launcher = TestFilesystemCheckLauncher::default();
    supervisor
        .launch_next_pending_filesystem_check_helper(&mut launcher, BOOT_NS + 1)
        .expect("launch helper")
        .expect("helper dispatch");
    let due_at_ns = supervisor
        .next_pre_start_check_timeout()
        .expect("helper timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();

    supervisor
        .process_due_filesystem_check_timeout(operation_id, due_at_ns, &mut controller)
        .expect("helper timeout")
        .expect("timeout dispatch");

    let generation_before = supervisor
        .services()
        .get("app")
        .expect("app")
        .runtime
        .cgroup_generation;

    // Still populated after the kill: D-state processes, nothing to be done.
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/app/checks", true);

    let leaks = supervisor
        .process_due_cgroup_cleanups(&mut controller, due_at_ns + 5_000_000_000)
        .expect("cleanup")
        .expect("a cleanup deadline was due");

    // The leak is reported out of the turn, not only recorded on the service:
    // a `status` poll finds the record, but nobody is necessarily polling the
    // service that leaked.
    assert_eq!(
        leaks
            .iter()
            .map(|leak| (leak.service.as_str(), leak.path.as_str(), leak.kind))
            .collect::<Vec<_>>(),
        vec![(
            "app",
            "/sys/fs/cgroup/peinit/app/checks",
            LeakedCgroupKind::Helper,
        )],
    );

    let runtime = &supervisor.services().get("app").expect("app").runtime;
    assert_eq!(
        runtime
            .leaked_cgroups
            .iter()
            .map(|leak| (leak.path.as_str(), leak.kind))
            .collect::<Vec<_>>(),
        vec![("/sys/fs/cgroup/peinit/app/checks", LeakedCgroupKind::Helper)],
    );
    // The bump is the load-bearing half: without it the next start reuses a
    // tree that still contains the unkillable process.
    assert!(
        runtime.cgroup_generation > generation_before,
        "a leaked helper cgroup must bump the generation",
    );
    // Abandoned, not removed -- removing a populated cgroup cannot work.
    assert!(controller.cgroup_removes.is_empty());
}

#[test]
fn timed_out_filesystem_condition_helper_records_cleanup_and_removes_empty_cgroup() {
    let mut app = alive_service("app");
    app.conditions = vec![ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/app".to_string(),
    }];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);

    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let operation_id = operation_for_service(&boot.plan, "app");
    let mut launcher = TestFilesystemCheckLauncher::default();
    supervisor
        .launch_next_pending_filesystem_check_helper(&mut launcher, BOOT_NS + 1)
        .expect("launch helper")
        .expect("helper dispatch");
    let due_at_ns = supervisor
        .next_pre_start_check_timeout()
        .expect("helper timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();

    supervisor
        .process_due_filesystem_check_timeout(operation_id, due_at_ns, &mut controller)
        .expect("helper timeout")
        .expect("timeout dispatch");
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/app/checks", false);

    supervisor
        .process_due_cgroup_cleanups(&mut controller, due_at_ns + 5_000_000_000)
        .expect("cleanup")
        .expect("a cleanup deadline was due");

    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/checks"]
    );
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/app/checks"]
    );
    assert_eq!(
        controller.cgroup_removes,
        vec!["/sys/fs/cgroup/peinit/app/checks"]
    );
}

#[test]
fn filesystem_root_condition_defers_dependency_until_helper_passes() {
    let mut app = alive_service("app");
    app.requires.push("db".to_string());
    let check = ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/app".to_string(),
    };
    app.conditions = vec![check.clone()];
    let mut db = alive_service("db");
    db.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, db]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let app_operation = operation_for_service(&boot.plan, "app");

    assert!(boot.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(
        supervisor.pending_pre_start_check_launches(),
        vec![app_operation]
    );
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Inactive,
    );

    let mut launcher = TestFilesystemCheckLauncher::default();
    supervisor
        .launch_next_pending_filesystem_check_helper(&mut launcher, BOOT_NS + 1)
        .expect("launch helper")
        .expect("helper dispatch");
    assert!(supervisor.pending_launch_jobs().is_empty());

    let completion = supervisor
        .complete_filesystem_check_helper(
            81,
            FilesystemCheckReport {
                service: "app".to_string(),
                operation_id: app_operation,
                results: vec![FilesystemCheckResult {
                    check,
                    satisfied: true,
                }],
            },
            BOOT_NS + 2,
        )
        .expect("complete helper");

    assert_eq!(completion.start_dispatches.len(), 1);
    assert_eq!(completion.start_dispatches[0].ready.service, "db");
    assert_eq!(
        supervisor.pending_launch_jobs(),
        vec![completion.start_dispatches[0].job_id],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );
}

#[test]
fn successful_filesystem_condition_helper_queues_service_job() {
    let mut app = alive_service("app");
    let check = ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/app".to_string(),
    };
    app.conditions = vec![check.clone()];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let operation_id = operation_for_service(&boot.plan, "app");

    let mut launcher = TestFilesystemCheckLauncher::default();
    supervisor
        .launch_next_pending_filesystem_check_helper(&mut launcher, BOOT_NS + 1)
        .expect("launch helper")
        .expect("helper dispatch");

    let completion = supervisor
        .complete_filesystem_check_helper(
            81,
            FilesystemCheckReport {
                service: "app".to_string(),
                operation_id,
                results: vec![FilesystemCheckResult {
                    check,
                    satisfied: true,
                }],
            },
            BOOT_NS + 1,
        )
        .expect("complete helper");

    assert!(completion.completion.job_id.is_none());
    let job_id = completion.start_dispatches[0].job_id;
    assert_eq!(supervisor.pending_launch_jobs(), vec![job_id]);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("operation")
            .state,
        OperationState::Running,
    );
}

fn registry_check(argument: &str) -> ServiceCheck {
    ServiceCheck {
        kind: ServiceCheckKind::Registry,
        argument: argument.to_string(),
    }
}

#[derive(Default)]
struct TestFilesystemCheckLauncher {
    requests: Vec<FilesystemCheckHelperRequest>,
}

impl FilesystemCheckHelperLauncher for TestFilesystemCheckLauncher {
    fn launch_filesystem_check_helper(
        &mut self,
        request: FilesystemCheckHelperRequest,
    ) -> Result<LaunchedFilesystemCheckHelper, BoundaryError> {
        self.requests.push(request.clone());
        Ok(LaunchedFilesystemCheckHelper {
            service: request.service,
            operation_id: request.operation_id,
            checks: request.checks,
            pid: 8001,
            pidfd: 80,
            result_fd: 81,
            cgroup_id: request.cgroup_id,
        })
    }
}

fn operation_for_service(
    plan: &crate::boot::phase2::Phase2BootPlan,
    service: &str,
) -> crate::ids::OperationId {
    plan.starts
        .iter()
        .find(|start| start.service == service)
        .expect("planned start")
        .operation_id
}
