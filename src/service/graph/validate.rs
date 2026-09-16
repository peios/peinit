use std::collections::{BTreeMap, BTreeSet};

use crate::service::{
    Readiness, ServiceDefinition, ServiceTrigger, ServiceType, hard_dependencies,
    is_valid_service_name,
};
use crate::timer::calendar::CalendarSchedule;

use crate::service::role::{AUTHN_ROLE, requires_authority, role_providers};
use crate::service::synthesise_role_dependencies;

use super::cycle::find_cycles;
use super::model::{
    ServiceGraphFinding, ServiceGraphValidation, ServiceGraphValidationFailure, ServiceGraphWarning,
};

/// Validate a definition set, as it will actually be executed.
///
/// The set is passed through role synthesis first, so validation sees the
/// same graph the supervisor will: a derived edge can close a cycle just as a
/// declared one can — an authority that `Requires` a service which is itself
/// non-SYSTEM is exactly that shape — and a cycle peinit only discovered at
/// boot would be a hang rather than a finding.
pub fn validate_service_graph(
    definitions: &[ServiceDefinition],
) -> Result<ServiceGraphValidation, ServiceGraphValidationFailure> {
    let definitions = &synthesise_role_dependencies(definitions.to_vec());
    let index = index_definitions(definitions);
    let mut findings = invalid_service_name_findings(definitions);
    findings.extend(index.duplicate_findings);
    findings.extend(missing_hard_dependencies(definitions, &index.by_name));
    findings.extend(conflicting_boot_services(&index.by_name));
    findings.extend(health_check_restart_window_findings(definitions));
    findings.extend(unschedulable_health_check_findings(definitions));
    findings.extend(invalid_timer_schedule_findings(definitions));
    findings.extend(cycle_findings(&index.by_name));

    // Warnings are computed whether or not there are findings. A boot does
    // not refuse a graph with findings: it blocks the services they name and
    // starts everything else, and a warning about everything else is exactly
    // what it then needs to say (PEI-1124). Computing them only on the clean
    // path meant one missing dependency anywhere silenced every warning.
    let mut warnings = readiness_warnings(&index.by_name);
    warnings.extend(unfilled_role_warnings(definitions));

    if !findings.is_empty() {
        return Err(ServiceGraphValidationFailure { findings, warnings });
    }

    Ok(ServiceGraphValidation {
        service_count: index.by_name.len(),
        warnings,
    })
}

struct DefinitionIndex<'a> {
    by_name: BTreeMap<&'a str, &'a ServiceDefinition>,
    duplicate_findings: Vec<ServiceGraphFinding>,
}

fn index_definitions(definitions: &[ServiceDefinition]) -> DefinitionIndex<'_> {
    let mut by_name = BTreeMap::new();
    let mut duplicates = BTreeSet::new();

    for definition in definitions {
        if by_name.contains_key(definition.name.as_str()) {
            duplicates.insert(definition.name.clone());
        } else {
            by_name.insert(definition.name.as_str(), definition);
        }
    }

    let duplicate_findings = duplicates
        .into_iter()
        .map(|service| ServiceGraphFinding::DuplicateService { service })
        .collect();
    DefinitionIndex {
        by_name,
        duplicate_findings,
    }
}

fn invalid_service_name_findings(definitions: &[ServiceDefinition]) -> Vec<ServiceGraphFinding> {
    definitions
        .iter()
        .filter(|definition| !is_valid_service_name(&definition.name))
        .map(|definition| ServiceGraphFinding::InvalidServiceName {
            service: definition.name.clone(),
        })
        .collect()
}

fn missing_hard_dependencies(
    definitions: &[ServiceDefinition],
    by_name: &BTreeMap<&str, &ServiceDefinition>,
) -> Vec<ServiceGraphFinding> {
    definitions
        .iter()
        .flat_map(|definition| {
            hard_dependencies(definition)
                .into_iter()
                .filter(|dependency| !by_name.contains_key(dependency.target.as_str()))
                .map(|dependency| ServiceGraphFinding::MissingHardDependency {
                    service: definition.name.clone(),
                    target: dependency.target,
                    kind: dependency.kind,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn conflicting_boot_services(
    by_name: &BTreeMap<&str, &ServiceDefinition>,
) -> Vec<ServiceGraphFinding> {
    let mut pairs = BTreeSet::<(String, String)>::new();
    for definition in by_name.values() {
        if !definition.has_boot_trigger() {
            continue;
        }
        for target in &definition.conflicts {
            let Some(target_definition) = by_name.get(target.as_str()) else {
                continue;
            };
            if !target_definition.has_boot_trigger() {
                continue;
            }
            let pair = if definition.name <= *target {
                (definition.name.clone(), target.clone())
            } else {
                (target.clone(), definition.name.clone())
            };
            pairs.insert(pair);
        }
    }
    pairs
        .into_iter()
        .map(|(service, target)| ServiceGraphFinding::ConflictingBootServices { service, target })
        .collect()
}

/// The flap constraint, `HealthCheckRetries * HealthCheckInterval <
/// RestartWindow`, applies only where a health check will actually run.
///
/// Health checks are scheduled for `ServiceType::Simple` alone, so a Oneshot
/// carrying one was being blocked at boot — or rejecting a whole reload — over
/// the interaction of two settings neither of which would ever be consulted.
/// The check could not run, so it could not flap, so it could not produce the
/// failure the constraint exists to prevent. The operator adjusted
/// `RestartWindow`, the definition validated, and the `HealthCheck` still did
/// nothing (PEI-367).
///
/// A non-Simple service declaring a `HealthCheck` is still a mistake, and
/// [`unschedulable_health_check_findings`] says so directly — which is the
/// thing actually wrong with that definition.
fn health_check_restart_window_findings(
    definitions: &[ServiceDefinition],
) -> Vec<ServiceGraphFinding> {
    definitions
        .iter()
        .filter(|definition| health_check_is_scheduled(definition))
        .filter(|definition| {
            health_check_failure_window_secs(definition) >= definition.restart_window_secs
        })
        .map(
            |definition| ServiceGraphFinding::InvalidHealthCheckRestartWindow {
                service: definition.name.clone(),
                retries: definition.health_check_retries,
                interval_secs: definition.health_check_interval_secs,
                restart_window_secs: definition.restart_window_secs,
            },
        )
        .collect()
}

/// A `HealthCheck` on a service that will never run one.
fn unschedulable_health_check_findings(
    definitions: &[ServiceDefinition],
) -> Vec<ServiceGraphFinding> {
    definitions
        .iter()
        .filter(|definition| {
            definition.health_check.is_some() && !health_check_is_scheduled(definition)
        })
        .map(|definition| ServiceGraphFinding::UnschedulableHealthCheck {
            service: definition.name.clone(),
            service_type: definition.service_type,
        })
        .collect()
}

/// Health checks are scheduled for Simple services only.
fn health_check_is_scheduled(definition: &ServiceDefinition) -> bool {
    definition.health_check.is_some() && definition.service_type == ServiceType::Simple
}

fn health_check_failure_window_secs(definition: &ServiceDefinition) -> u64 {
    u64::from(definition.health_check_retries).saturating_mul(definition.health_check_interval_secs)
}

fn invalid_timer_schedule_findings(definitions: &[ServiceDefinition]) -> Vec<ServiceGraphFinding> {
    definitions
        .iter()
        .flat_map(|definition| {
            definition.triggers.iter().filter_map(|trigger| {
                let ServiceTrigger::Timer { schedule } = trigger else {
                    return None;
                };
                CalendarSchedule::parse(schedule).err().map(|error| {
                    ServiceGraphFinding::InvalidTimerSchedule {
                        service: definition.name.clone(),
                        schedule: schedule.clone(),
                        message: error.to_string(),
                    }
                })
            })
        })
        .collect()
}

fn cycle_findings(by_name: &BTreeMap<&str, &ServiceDefinition>) -> Vec<ServiceGraphFinding> {
    find_cycles(by_name)
        .into_iter()
        .map(|services| ServiceGraphFinding::Cycle { services })
        .collect()
}

fn readiness_warnings(by_name: &BTreeMap<&str, &ServiceDefinition>) -> Vec<ServiceGraphWarning> {
    let mut dependents = BTreeMap::<String, Vec<String>>::new();
    for definition in by_name.values() {
        for dependency in hard_dependencies(definition) {
            if by_name.contains_key(dependency.target.as_str()) {
                dependents
                    .entry(dependency.target)
                    .or_default()
                    .push(definition.name.clone());
            }
        }
    }

    by_name
        .values()
        .filter(|definition| definition.readiness == Readiness::Alive)
        .filter_map(|definition| {
            dependents
                .get(definition.name.as_str())
                .cloned()
                .filter(|entries| !entries.is_empty())
                .map(
                    |dependents| ServiceGraphWarning::AliveReadinessWithHardDependents {
                        service: definition.name.clone(),
                        dependents,
                    },
                )
        })
        .collect()
}

/// Services that cannot start because nothing fills the role they need.
///
/// This is the case role synthesis deliberately declines to express as an
/// edge (see [`crate::service::role`]): inventing a dependency on a name no
/// service answers to would make every reload of this image fail validation,
/// including the reload that would install the missing authority.
fn unfilled_role_warnings(definitions: &[ServiceDefinition]) -> Vec<ServiceGraphWarning> {
    if !role_providers(definitions, AUTHN_ROLE).is_empty() {
        return Vec::new();
    }
    let services: Vec<String> = definitions
        .iter()
        .filter(|definition| requires_authority(definition))
        .map(|definition| definition.name.clone())
        .collect();
    if services.is_empty() {
        return Vec::new();
    }
    vec![ServiceGraphWarning::UnfilledRole {
        role: AUTHN_ROLE.to_string(),
        services,
    }]
}
