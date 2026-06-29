use std::collections::{BTreeMap, BTreeSet};

use crate::boot::phase2::BlockedReason;
use crate::service::{ErrorControl, ServiceDefinition};

use super::blocked::block_service;
use super::model::{BlockedServiceDraft, ServiceMap};

pub(in crate::boot::phase2::graph) fn block_unresolvable_conflicts(
    included: &BTreeSet<String>,
    by_name: &ServiceMap<'_>,
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
) {
    for service in included {
        let Some(definition) = by_name.get(service.as_str()).copied() else {
            continue;
        };
        if !definition.has_boot_trigger() {
            continue;
        }
        for target in symmetric_conflicts(definition, by_name) {
            if !included.contains(&target) {
                continue;
            }
            let Some(target_definition) = by_name.get(target.as_str()).copied() else {
                continue;
            };
            if !target_definition.has_boot_trigger() {
                continue;
            }
            block_service(
                blocked,
                service,
                BlockedReason::ConflictingBootService {
                    target: target.clone(),
                },
            );
            block_service(
                blocked,
                &target,
                BlockedReason::ConflictingBootService {
                    target: service.clone(),
                },
            );
        }
    }
}

pub(in crate::boot::phase2::graph) fn critical_boot_conflict_exists(
    included: &BTreeSet<String>,
    by_name: &ServiceMap<'_>,
) -> bool {
    included.iter().any(|service| {
        let Some(definition) = by_name.get(service.as_str()).copied() else {
            return false;
        };
        definition.has_boot_trigger()
            && is_critical(definition)
            && symmetric_conflicts(definition, by_name)
                .into_iter()
                .any(|target| {
                    included.contains(&target)
                        && by_name
                            .get(target.as_str())
                            .is_some_and(|target_definition| target_definition.has_boot_trigger())
                })
    })
}

fn symmetric_conflicts(definition: &ServiceDefinition, by_name: &ServiceMap<'_>) -> Vec<String> {
    let mut conflicts = definition.conflicts.clone();
    for other in by_name.values() {
        if other.name != definition.name && other.conflicts.contains(&definition.name) {
            conflicts.push(other.name.clone());
        }
    }
    conflicts.sort();
    conflicts.dedup();
    conflicts
}

pub(in crate::boot::phase2::graph) fn is_critical(definition: &ServiceDefinition) -> bool {
    definition.error_control == ErrorControl::Critical
}
