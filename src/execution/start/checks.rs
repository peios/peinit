use crate::boundary::FilesystemCheckResult;
use crate::service::{ServiceCheck, ServiceCheckKind, ServiceTable};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreStartCheckDecision {
    Passed,
    ConditionSkipped(ServiceCheck),
    AssertionFailed(ServiceCheck),
    RequiresFilesystemHelper { checks: Vec<ServiceCheck> },
}

/// Decide what a service's conditions and asserts require before it can start.
///
/// Filesystem checks are gathered from **both** lists into a single helper run.
/// There is only ever one: the completion path evaluates conditions and asserts
/// together against the results it gets back, and there is no mechanism to ask
/// for a second round. Returning after the conditions' filesystem checks — as
/// this did until PEI-342 — meant the helper never stat'd the asserts' paths, so
/// every one of them evaluated unsatisfied and the service failed
/// `AssertionError` at every start against an assert that was in fact met.
///
/// A registry check failing still short-circuits, and the conditions still take
/// precedence: a failing *condition* skips the service, which is not the same
/// outcome as a failing assert. A failing registry *assert* is only reported
/// here when no helper is needed at all — otherwise the conditions have not been
/// evaluated yet, and a condition that turns out to be unmet must skip rather
/// than fail. The completion path re-evaluates both lists in order and reaches
/// the same assert.
pub(super) fn evaluate_cacheable_pre_start_checks(
    services: &ServiceTable,
    conditions: &[ServiceCheck],
    asserts: &[ServiceCheck],
) -> PreStartCheckDecision {
    let mut filesystem_checks = Vec::new();

    match evaluate_check_set(services, conditions) {
        CheckSetDecision::Passed => {}
        CheckSetDecision::Failed(check) => return PreStartCheckDecision::ConditionSkipped(check),
        CheckSetDecision::RequiresFilesystemHelper { checks } => {
            extend_filesystem_checks(&mut filesystem_checks, checks);
        }
    }

    match evaluate_check_set(services, asserts) {
        CheckSetDecision::Passed => {}
        CheckSetDecision::Failed(check) => {
            if filesystem_checks.is_empty() {
                return PreStartCheckDecision::AssertionFailed(check);
            }
        }
        CheckSetDecision::RequiresFilesystemHelper { checks } => {
            extend_filesystem_checks(&mut filesystem_checks, checks);
        }
    }

    if filesystem_checks.is_empty() {
        PreStartCheckDecision::Passed
    } else {
        PreStartCheckDecision::RequiresFilesystemHelper {
            checks: filesystem_checks,
        }
    }
}

/// Append checks the helper has not already been asked for.
///
/// The same path may legitimately appear in both lists, and the helper stats
/// each entry it is given — so without this a duplicated path costs a second
/// stat for a result that is looked up by equality anyway.
fn extend_filesystem_checks(gathered: &mut Vec<ServiceCheck>, checks: Vec<ServiceCheck>) {
    for check in checks {
        if !gathered.contains(&check) {
            gathered.push(check);
        }
    }
}

pub(super) fn evaluate_pre_start_checks_with_filesystem_results(
    services: &ServiceTable,
    conditions: &[ServiceCheck],
    asserts: &[ServiceCheck],
    results: &[FilesystemCheckResult],
) -> PreStartCheckDecision {
    if let Some(check) = failed_check_with_filesystem_results(services, conditions, results) {
        return PreStartCheckDecision::ConditionSkipped(check);
    }
    if let Some(check) = failed_check_with_filesystem_results(services, asserts, results) {
        return PreStartCheckDecision::AssertionFailed(check);
    }
    PreStartCheckDecision::Passed
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

/// The first check in `checks` that the helper's results do not satisfy.
///
/// Returns an `Option` rather than a `CheckSetDecision` because once results are
/// in hand there is no third outcome: a filesystem check with no result fails
/// closed. Modelling it as a decision left two arms handling a
/// `RequiresFilesystemHelper` that could never be returned, which is what made
/// `UnexpectedFilesystemCheckContinuation` unreachable and hid PEI-342.
fn failed_check_with_filesystem_results(
    services: &ServiceTable,
    checks: &[ServiceCheck],
    results: &[FilesystemCheckResult],
) -> Option<ServiceCheck> {
    checks.iter().find_map(|check| {
        let satisfied = match check.kind {
            ServiceCheckKind::Registry => cached_registry_key_exists(services, &check.argument),
            ServiceCheckKind::Path | ServiceCheckKind::File | ServiceCheckKind::Directory => {
                filesystem_check_satisfied(check, results)
            }
        };
        (!satisfied).then(|| check.clone())
    })
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
