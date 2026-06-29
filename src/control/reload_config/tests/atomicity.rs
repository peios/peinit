use crate::boundary::BoundaryError;
use crate::control::reload_config::{ReloadConfigError, reload_config};
use crate::service::{ServiceDependencyKind, ServiceGraphFinding};

use super::{StaticRegistry, service, service_table};

#[test]
fn registry_read_failure_leaves_service_table_unchanged() {
    let mut services = service_table(&["app"]);
    let original = services.clone();
    let mut registry = StaticRegistry::error(BoundaryError::Registry("offline".to_string()));

    let error = reload_config(&mut registry, &mut services).expect_err("registry error");

    assert_eq!(
        error,
        ReloadConfigError::Registry(BoundaryError::Registry("offline".to_string()))
    );
    assert_eq!(services, original);
}

#[test]
fn missing_hard_dependency_leaves_service_table_unchanged() {
    let mut services = service_table(&["app"]);
    let original = services.clone();
    let mut app = service("app", "/sbin/app-v2");
    app.requires.push("db".to_string());
    let mut registry = StaticRegistry::services(vec![app]);

    let error = reload_config(&mut registry, &mut services).expect_err("validation error");

    assert_eq!(services, original);
    assert_eq!(
        validation_findings(error),
        vec![ServiceGraphFinding::MissingHardDependency {
            service: "app".to_string(),
            target: "db".to_string(),
            kind: ServiceDependencyKind::Requires,
        }]
    );
}

#[test]
fn dependency_cycle_leaves_service_table_unchanged() {
    let mut services = service_table(&["app"]);
    let original = services.clone();
    let mut app = service("app", "/sbin/app-v2");
    app.requires.push("db".to_string());
    let mut db = service("db", "/sbin/db");
    db.requires.push("app".to_string());
    let mut registry = StaticRegistry::services(vec![app, db]);

    let error = reload_config(&mut registry, &mut services).expect_err("validation error");

    assert_eq!(services, original);
    assert_eq!(
        validation_findings(error),
        vec![ServiceGraphFinding::Cycle {
            services: vec!["app".to_string(), "db".to_string()],
        }]
    );
}

#[test]
fn duplicate_services_leave_service_table_unchanged() {
    let mut services = service_table(&["app"]);
    let original = services.clone();
    let mut registry = StaticRegistry::services(vec![
        service("app", "/sbin/app-v2"),
        service("app", "/sbin/other"),
    ]);

    let error = reload_config(&mut registry, &mut services).expect_err("validation error");

    assert_eq!(services, original);
    assert_eq!(
        validation_findings(error),
        vec![ServiceGraphFinding::DuplicateService {
            service: "app".to_string(),
        }]
    );
}

fn validation_findings(error: ReloadConfigError) -> Vec<ServiceGraphFinding> {
    match error {
        ReloadConfigError::Validation(failure) => failure.findings,
        other => panic!("expected validation error, got {other:?}"),
    }
}
