use crate::boundary::FilesystemCheckResult;
use crate::service::{ServiceCheck, ServiceCheckKind, ServiceTable};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreStartCheckDecision {
    Passed,
    ConditionSkipped(ServiceCheck),
    AssertionFailed(ServiceCheck),
    RequiresFilesystemHelper { checks: Vec<ServiceCheck> },
}

pub(super) fn evaluate_cacheable_pre_start_checks(
    services: &ServiceTable,
    conditions: &[ServiceCheck],
    asserts: &[ServiceCheck],
) -> PreStartCheckDecision {
    match evaluate_check_set(services, conditions) {
        CheckSetDecision::Passed => {}
        CheckSetDecision::Failed(check) => return PreStartCheckDecision::ConditionSkipped(check),
        CheckSetDecision::RequiresFilesystemHelper { checks } => {
            return PreStartCheckDecision::RequiresFilesystemHelper { checks };
        }
    }

    match evaluate_check_set(services, asserts) {
        CheckSetDecision::Passed => PreStartCheckDecision::Passed,
        CheckSetDecision::Failed(check) => PreStartCheckDecision::AssertionFailed(check),
        CheckSetDecision::RequiresFilesystemHelper { checks } => {
            PreStartCheckDecision::RequiresFilesystemHelper { checks }
        }
    }
}

pub(super) fn evaluate_pre_start_checks_with_filesystem_results(
    services: &ServiceTable,
    conditions: &[ServiceCheck],
    asserts: &[ServiceCheck],
    results: &[FilesystemCheckResult],
) -> PreStartCheckDecision {
    match evaluate_check_set_with_filesystem_results(services, conditions, results) {
        CheckSetDecision::Passed => {}
        CheckSetDecision::Failed(check) => return PreStartCheckDecision::ConditionSkipped(check),
        CheckSetDecision::RequiresFilesystemHelper { .. } => {
            return PreStartCheckDecision::RequiresFilesystemHelper { checks: Vec::new() };
        }
    }

    match evaluate_check_set_with_filesystem_results(services, asserts, results) {
        CheckSetDecision::Passed => PreStartCheckDecision::Passed,
        CheckSetDecision::Failed(check) => PreStartCheckDecision::AssertionFailed(check),
        CheckSetDecision::RequiresFilesystemHelper { .. } => {
            PreStartCheckDecision::RequiresFilesystemHelper { checks: Vec::new() }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CheckSetDecision {
    Passed,
    Failed(ServiceCheck),
    RequiresFilesystemHelper { checks: Vec<ServiceCheck> },
}

fn evaluate_check_set(services: &ServiceTable, checks: &[ServiceCheck]) -> CheckSetDecision {
    let mut filesystem_checks = Vec::new();
    for check in checks {
        match check.kind {
            ServiceCheckKind::Registry => {
                if !cached_registry_key_exists(services, &check.argument) {
                    return CheckSetDecision::Failed(check.clone());
                }
            }
            ServiceCheckKind::Path | ServiceCheckKind::File | ServiceCheckKind::Directory => {
                filesystem_checks.push(check.clone());
            }
        }
    }
    if !filesystem_checks.is_empty() {
        return CheckSetDecision::RequiresFilesystemHelper {
            checks: filesystem_checks,
        };
    }
    CheckSetDecision::Passed
}

fn evaluate_check_set_with_filesystem_results(
    services: &ServiceTable,
    checks: &[ServiceCheck],
    results: &[FilesystemCheckResult],
) -> CheckSetDecision {
    for check in checks {
        let satisfied = match check.kind {
            ServiceCheckKind::Registry => cached_registry_key_exists(services, &check.argument),
            ServiceCheckKind::Path | ServiceCheckKind::File | ServiceCheckKind::Directory => {
                filesystem_check_satisfied(check, results)
            }
        };
        if !satisfied {
            return CheckSetDecision::Failed(check.clone());
        }
    }
    CheckSetDecision::Passed
}

fn filesystem_check_satisfied(check: &ServiceCheck, results: &[FilesystemCheckResult]) -> bool {
    results
        .iter()
        .find(|result| result.check == *check)
        .is_some_and(|result| result.satisfied)
}

fn cached_registry_key_exists(services: &ServiceTable, key: &str) -> bool {
    const SERVICES_ROOT: &str = "Machine\\System\\Services";
    const INIT_ROOT: &str = "Machine\\System\\Init";

    if key == SERVICES_ROOT || key == INIT_ROOT {
        return true;
    }

    let Some(service) = key.strip_prefix("Machine\\System\\Services\\") else {
        return false;
    };
    if service.is_empty() || service.contains('\\') {
        return false;
    }
    services.definition(service).is_some()
}

pub(super) fn format_check(check: &ServiceCheck) -> String {
    let kind = match check.kind {
        ServiceCheckKind::Path => "path",
        ServiceCheckKind::File => "file",
        ServiceCheckKind::Directory => "directory",
        ServiceCheckKind::Registry => "registry",
    };
    format!("{kind}:{}", check.argument)
}

#[cfg(test)]
mod tests;
