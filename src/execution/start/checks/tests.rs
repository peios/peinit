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

#[test]
fn filesystem_conditions_and_asserts_are_gathered_into_one_helper_run() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");

    let condition = ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/data".to_string(),
    };
    let assertion = ServiceCheck {
        kind: ServiceCheckKind::File,
        argument: "/etc/app/licence.key".to_string(),
    };

    // PEI-342: the assert's path used to be left out of the helper's work, so
    // it came back with no result and failed closed at every start.
    assert_eq!(
        evaluate_cacheable_pre_start_checks(
            &services,
            std::slice::from_ref(&condition),
            std::slice::from_ref(&assertion),
        ),
        PreStartCheckDecision::RequiresFilesystemHelper {
            checks: vec![condition.clone(), assertion.clone()],
        }
    );

    // ...and with both stat'd and satisfied, the service starts.
    assert_eq!(
        super::evaluate_pre_start_checks_with_filesystem_results(
            &services,
            std::slice::from_ref(&condition),
            std::slice::from_ref(&assertion),
            &[
                FilesystemCheckResult {
                    check: condition.clone(),
                    satisfied: true,
                },
                FilesystemCheckResult {
                    check: assertion.clone(),
                    satisfied: true,
                },
            ],
        ),
        PreStartCheckDecision::Passed
    );
}

#[test]
fn a_path_in_both_lists_is_only_stated_once() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");

    let shared = ServiceCheck {
        kind: ServiceCheckKind::Directory,
        argument: "/srv/data".to_string(),
    };

    assert_eq!(
        evaluate_cacheable_pre_start_checks(
            &services,
            std::slice::from_ref(&shared),
            std::slice::from_ref(&shared),
        ),
        PreStartCheckDecision::RequiresFilesystemHelper {
            checks: vec![shared],
        }
    );
}

#[test]
fn a_failing_registry_assert_defers_to_the_helper_when_conditions_need_one() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");

    let condition = ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/data".to_string(),
    };
    let assertion = registry_check("Machine\\System\\Services\\missing");

    // The assert is already known to fail, but the conditions have not been
    // evaluated yet — and an unmet condition skips rather than fails, so the
    // helper must run before either outcome can be named.
    assert_eq!(
        evaluate_cacheable_pre_start_checks(
            &services,
            std::slice::from_ref(&condition),
            std::slice::from_ref(&assertion),
        ),
        PreStartCheckDecision::RequiresFilesystemHelper {
            checks: vec![condition.clone()],
        }
    );

    // An unmet condition wins...
    assert_eq!(
        super::evaluate_pre_start_checks_with_filesystem_results(
            &services,
            std::slice::from_ref(&condition),
            std::slice::from_ref(&assertion),
            &[FilesystemCheckResult {
                check: condition.clone(),
                satisfied: false,
            }],
        ),
        PreStartCheckDecision::ConditionSkipped(condition.clone())
    );

    // ...and once it is met, the assert is reported.
    assert_eq!(
        super::evaluate_pre_start_checks_with_filesystem_results(
            &services,
            std::slice::from_ref(&condition),
            std::slice::from_ref(&assertion),
            &[FilesystemCheckResult {
                check: condition.clone(),
                satisfied: true,
            }],
        ),
        PreStartCheckDecision::AssertionFailed(assertion)
    );
}

#[test]
fn filesystem_asserts_alone_still_require_the_helper() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");

    let assertion = ServiceCheck {
        kind: ServiceCheckKind::File,
        argument: "/etc/app/licence.key".to_string(),
    };

    assert_eq!(
        evaluate_cacheable_pre_start_checks(&services, &[], std::slice::from_ref(&assertion)),
        PreStartCheckDecision::RequiresFilesystemHelper {
            checks: vec![assertion],
        }
    );
}
