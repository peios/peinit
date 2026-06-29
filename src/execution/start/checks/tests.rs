use crate::boundary::FilesystemCheckResult;
use crate::service::{ServiceCheck, ServiceCheckKind, ServiceDefinition, ServiceTable};

use super::{
    PreStartCheckDecision, cached_registry_key_exists, evaluate_cacheable_pre_start_checks,
};

#[test]
fn cached_registry_service_keys_are_resolved_from_service_table() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");

    assert!(cached_registry_key_exists(
        &services,
        "Machine\\System\\Services"
    ));
    assert!(cached_registry_key_exists(
        &services,
        "Machine\\System\\Services\\app"
    ));
    assert!(!cached_registry_key_exists(
        &services,
        "Machine\\System\\Services\\missing"
    ));
    assert!(!cached_registry_key_exists(
        &services,
        "Machine\\System\\Services\\app\\child"
    ));
}

#[test]
fn conditions_short_circuit_before_asserts() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");

    let condition = registry_check("Machine\\System\\Services\\missing");
    let assertion = registry_check("Machine\\System\\Services\\also-missing");

    assert_eq!(
        evaluate_cacheable_pre_start_checks(
            &services,
            std::slice::from_ref(&condition),
            &[assertion]
        ),
        PreStartCheckDecision::ConditionSkipped(condition)
    );
}

#[test]
fn filesystem_checks_require_helper() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");

    assert_eq!(
        evaluate_cacheable_pre_start_checks(
            &services,
            &[ServiceCheck {
                kind: ServiceCheckKind::Path,
                argument: "/srv/app".to_string(),
            }],
            &[],
        ),
        PreStartCheckDecision::RequiresFilesystemHelper {
            checks: vec![ServiceCheck {
                kind: ServiceCheckKind::Path,
                argument: "/srv/app".to_string(),
            }],
        }
    );
}

#[test]
fn filesystem_results_fail_closed_when_missing_or_false() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");
    let check = ServiceCheck {
        kind: ServiceCheckKind::File,
        argument: "/srv/app/config".to_string(),
    };

    assert_eq!(
        super::evaluate_pre_start_checks_with_filesystem_results(
            &services,
            std::slice::from_ref(&check),
            &[],
            &[],
        ),
        PreStartCheckDecision::ConditionSkipped(check.clone())
    );
    assert_eq!(
        super::evaluate_pre_start_checks_with_filesystem_results(
            &services,
            std::slice::from_ref(&check),
            &[],
            &[FilesystemCheckResult {
                check: check.clone(),
                satisfied: false,
            }],
        ),
        PreStartCheckDecision::ConditionSkipped(check)
    );
}

fn registry_check(argument: &str) -> ServiceCheck {
    ServiceCheck {
        kind: ServiceCheckKind::Registry,
        argument: argument.to_string(),
    }
}
