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

/// Every critical boot conflict in the included set, as (service, target).
///
/// Collected rather than merely detected: these are the findings that force a
/// Safe-mode downgrade, and an operator whose machine came up in Safe mode
/// needs to know which services caused it. Deduplicated by ordered pair, so a
/// symmetric conflict is reported once rather than from both ends.
pub(in crate::boot::phase2::graph) fn critical_boot_conflicts(
    included: &BTreeSet<String>,
    by_name: &ServiceMap<'_>,
) -> Vec<(String, String)> {
    let mut found = BTreeSet::new();
    for service in included {
        let Some(definition) = by_name.get(service.as_str()).copied() else {
            continue;
        };
        if !definition.has_boot_trigger() || !is_critical(definition) {
            continue;
        }
        for target in symmetric_conflicts(definition, by_name) {
            if !included.contains(&target) {
                continue;
            }
            if by_name
                .get(target.as_str())
                .is_some_and(|target_definition| target_definition.has_boot_trigger())
            {
                let pair = if definition.name <= target {
                    (definition.name.clone(), target)
                } else {
                    (target, definition.name.clone())
                };
                found.insert(pair);
            }
        }
    }
    found.into_iter().collect()
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
