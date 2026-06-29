use std::collections::{BTreeMap, BTreeSet};

use crate::service::{
    Readiness, ServiceDefinition, ServiceTrigger, hard_dependencies, is_valid_service_name,
};
use crate::timer::calendar::CalendarSchedule;

use super::cycle::find_cycles;
use super::model::{
    ServiceGraphFinding, ServiceGraphValidation, ServiceGraphValidationFailure, ServiceGraphWarning,
};

pub fn validate_service_graph(
    definitions: &[ServiceDefinition],
) -> Result<ServiceGraphValidation, ServiceGraphValidationFailure> {
    let index = index_definitions(definitions);
    let mut findings = invalid_service_name_findings(definitions);
    findings.extend(index.duplicate_findings);
    findings.extend(missing_hard_dependencies(definitions, &index.by_name));
    findings.extend(conflicting_boot_services(&index.by_name));
    findings.extend(health_check_restart_window_findings(definitions));
    findings.extend(invalid_timer_schedule_findings(definitions));
    findings.extend(cycle_findings(&index.by_name));

    if !findings.is_empty() {
        return Err(ServiceGraphValidationFailure { findings });
    }

    Ok(ServiceGraphValidation {
        service_count: index.by_name.len(),
        warnings: readiness_warnings(&index.by_name),
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

fn health_check_restart_window_findings(
    definitions: &[ServiceDefinition],
) -> Vec<ServiceGraphFinding> {
    definitions
        .iter()
        .filter(|definition| definition.health_check.is_some())
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
