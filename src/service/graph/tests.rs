use crate::service::{
    Readiness, ServiceDefinition, ServiceDependencyKind, ServiceGraphFinding, ServiceGraphWarning,
    ServiceTrigger, ServiceType, validate_service_graph,
};

fn service(name: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"))
}

#[test]
fn valid_graph_reports_service_count_and_readiness_warnings() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut db = service("db");
    db.readiness = Readiness::Alive;

    let validation = validate_service_graph(&[app, db]).expect("valid graph");

    assert_eq!(validation.service_count, 2);
    assert_eq!(
        validation.warnings,
        vec![ServiceGraphWarning::AliveReadinessWithHardDependents {
            service: "db".to_string(),
            dependents: vec!["app".to_string()],
        }]
    );
}

#[test]
fn duplicate_service_names_are_validation_findings() {
    let failure = validate_service_graph(&[service("app"), service("app")]).expect_err("duplicate");

    assert_eq!(
        failure.findings,
        vec![ServiceGraphFinding::DuplicateService {
            service: "app".to_string(),
        }]
    );
}

#[test]
fn invalid_service_names_are_validation_findings() {
    let failure = validate_service_graph(&[service("bad/name")]).expect_err("invalid service name");

    assert_eq!(
        failure.findings,
        vec![ServiceGraphFinding::InvalidServiceName {
            service: "bad/name".to_string(),
        }]
    );
}

#[test]
fn missing_requires_and_binds_to_targets_are_validation_findings() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    app.binds_to.push("network".to_string());

    let failure = validate_service_graph(&[app]).expect_err("missing hard deps");

    assert_eq!(
        failure.findings,
        vec![
            ServiceGraphFinding::MissingHardDependency {
                service: "app".to_string(),
                target: "db".to_string(),
                kind: ServiceDependencyKind::Requires,
            },
            ServiceGraphFinding::MissingHardDependency {
                service: "app".to_string(),
                target: "network".to_string(),
                kind: ServiceDependencyKind::BindsTo,
            },
        ]
    );
}

#[test]
fn missing_wants_targets_are_ignored() {
    let mut app = service("app");
    app.wants.push("metrics".to_string());

    let validation = validate_service_graph(&[app]).expect("missing wants ignored");

    assert_eq!(validation.service_count, 1);
}

#[test]
fn missing_conflict_targets_are_ignored() {
    let mut app = service("app");
    app.conflicts.push("missing".to_string());

    let validation = validate_service_graph(&[app]).expect("missing conflicts ignored");

    assert_eq!(validation.service_count, 1);
}

#[test]
fn boot_triggered_conflicts_are_validation_findings() {
    let mut app = service("app");
    app.conflicts.push("legacy".to_string());
    let legacy = service("legacy");

    let failure = validate_service_graph(&[app, legacy]).expect_err("boot conflict");

    assert_eq!(
        failure.findings,
        vec![ServiceGraphFinding::ConflictingBootServices {
            service: "app".to_string(),
            target: "legacy".to_string(),
        }]
    );
}

#[test]
fn demand_only_conflicts_are_not_boot_validation_findings() {
    let mut app = service("app");
    app.triggers.clear();
    app.conflicts.push("legacy".to_string());
    let legacy = service("legacy");

    let validation = validate_service_graph(&[app, legacy]).expect("demand conflict allowed");

    assert_eq!(validation.service_count, 2);
}

#[test]
fn invalid_health_check_restart_windows_are_validation_findings() {
    let mut app = service("app");
    app.health_check = Some("/bin/check".to_string());
    app.health_check_retries = 3;
    app.health_check_interval_secs = 10;
    app.restart_window_secs = 30;

    let failure = validate_service_graph(&[app]).expect_err("invalid health timing");

    assert_eq!(
        failure.findings,
        vec![ServiceGraphFinding::InvalidHealthCheckRestartWindow {
            service: "app".to_string(),
            retries: 3,
            interval_secs: 10,
            restart_window_secs: 30,
        }]
    );
}

#[test]
fn invalid_timer_schedules_are_validation_findings() {
    let mut app = service("app");
    app.triggers.push(ServiceTrigger::Timer {
        schedule: "*-*-* 12:00:00.5 UTC".to_string(),
    });

    let failure = validate_service_graph(&[app]).expect_err("invalid timer schedule");

    assert_eq!(failure.findings.len(), 1);
    let ServiceGraphFinding::InvalidTimerSchedule {
        service,
        schedule,
        message,
    } = &failure.findings[0]
    else {
        panic!("expected invalid timer schedule finding");
    };
    assert_eq!(service, "app");
    assert_eq!(schedule, "*-*-* 12:00:00.5 UTC");
    assert!(message.contains("fractional seconds"));
}

#[test]
fn cycles_across_ordering_dependencies_are_validation_findings() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut db = service("db");
    db.wants.push("app".to_string());

    let failure = validate_service_graph(&[app, db]).expect_err("cycle");

    assert_eq!(
        failure.findings,
        vec![ServiceGraphFinding::Cycle {
            services: vec!["app".to_string(), "db".to_string()],
        }]
    );
}

#[test]
fn self_references_are_reported_as_cycles() {
    let mut app = service("app");
    app.requires.push("app".to_string());

    let failure = validate_service_graph(&[app]).expect_err("self reference");

    assert_eq!(
        failure.findings,
        vec![ServiceGraphFinding::Cycle {
            services: vec!["app".to_string()],
        }]
    );
}

// PEI-367. The flap constraint `HealthCheckRetries * HealthCheckInterval <
// RestartWindow` exists because a configuration violating it restarts
// indefinitely. Health checks are scheduled for Simple services only, so a
// Oneshot carrying one cannot flap — but the validation filtered on
// `health_check.is_some()` alone, so such a definition was blocked at boot, or
// rejected a whole reload, over the interaction of two settings neither of
// which would ever be consulted.
//
// A Oneshot with a HealthCheck *is* a mistake. Saying so via timing arithmetic
// is not: the operator adjusts RestartWindow, the definition validates, and the
// check still does nothing.
#[test]
fn a_oneshot_health_check_is_reported_as_unschedulable_not_as_bad_timing() {
    let mut task = service("task");
    task.service_type = ServiceType::Oneshot;
    task.triggers.clear();
    task.health_check = Some("/bin/probe".to_string());
    // Timing that would trip the flap constraint if it applied.
    task.health_check_retries = 10;
    task.health_check_interval_secs = 60;
    task.restart_window_secs = 30;

    let failure = validate_service_graph(&[task]).expect_err("a oneshot health check is invalid");

    assert_eq!(
        failure.findings,
        vec![ServiceGraphFinding::UnschedulableHealthCheck {
            service: "task".to_string(),
            service_type: ServiceType::Oneshot,
        }],
    );
}

/// A Simple service is still held to the constraint. Narrowing it keeps it
/// applying exactly where a check can flap.
#[test]
fn a_simple_service_still_fails_the_flap_constraint() {
    let mut app = service("app");
    app.health_check = Some("/bin/probe".to_string());
    app.health_check_retries = 10;
    app.health_check_interval_secs = 60;
    app.restart_window_secs = 30;

    let failure = validate_service_graph(&[app]).expect_err("flap constraint");

    assert_eq!(
        failure.findings,
        vec![ServiceGraphFinding::InvalidHealthCheckRestartWindow {
            service: "app".to_string(),
            retries: 10,
            interval_secs: 60,
            restart_window_secs: 30,
        }],
    );
}
