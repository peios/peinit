use std::collections::{BTreeMap, BTreeSet};

use crate::boot::BootMode;
use crate::boot::phase2::BlockedReason;
use crate::service::{ErrorControl, ServiceDefinition, hard_dependencies};

use super::blocked::block_service;
use super::model::{BlockedServiceDraft, ServiceMap};

pub(super) struct BootClosure {
    pub included: BTreeSet<String>,
    pub blocked: BTreeMap<String, BlockedServiceDraft>,
}

pub(super) fn collect_boot_closure(
    mode: BootMode,
    services: &[ServiceDefinition],
    by_name: &ServiceMap<'_>,
) -> BootClosure {
    let roots = boot_roots(services, mode);
    let mut included = BTreeSet::new();
    let mut blocked = BTreeMap::new();

    for root in roots {
        include_closure(root, mode, by_name, &mut included, &mut blocked);
    }
    propagate_blocked(&included, by_name, &mut blocked);

    BootClosure { included, blocked }
}

fn boot_roots(services: &[ServiceDefinition], mode: BootMode) -> Vec<&str> {
    services
        .iter()
        .filter(|service| !service.disabled)
        .filter(|service| mode == BootMode::Full || dependency_eligible(mode, service))
        .filter(|service| service.has_boot_trigger())
        .map(|service| service.name.as_str())
        .collect()
}

fn include_closure(
    service: &str,
    mode: BootMode,
    by_name: &ServiceMap<'_>,
    included: &mut BTreeSet<String>,
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
) {
    if !included.insert(service.to_string()) {
        return;
    }
    let Some(definition) = by_name.get(service).copied() else {
        return;
    };

    for dependency in hard_dependencies(definition) {
        match by_name.get(dependency.target.as_str()).copied() {
            Some(target_definition)
                if !target_definition.disabled && dependency_eligible(mode, target_definition) =>
            {
                include_closure(&target_definition.name, mode, by_name, included, blocked);
            }
            _ => {
                if mode == BootMode::Full {
                    // Through block_service, not or_insert: a service can be
                    // missing more than one hard dependency, and or_insert kept
                    // whichever was found first (PSD-007 §6.2 retention).
                    block_service(
                        blocked,
                        &definition.name,
                        BlockedReason::HardDependencyUnavailable {
                            target: dependency.target,
                            kind: dependency.kind,
                        },
                    );
                }
            }
        }
    }
    for declared in &definition.wants {
        // A `Wants` may name a level; the graph is keyed by service name.
        let (target, _level) = crate::service::split_target(declared);
        if let Some(target_definition) = by_name.get(target.as_str()).copied()
            && !target_definition.disabled
            && dependency_eligible(mode, target_definition)
        {
            include_closure(&target_definition.name, mode, by_name, included, blocked);
        }
    }
}

pub(super) fn propagate_blocked(
    included: &BTreeSet<String>,
    by_name: &ServiceMap<'_>,
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
) {
    let mut changed = true;
    while changed {
        changed = false;
        for service in included {
            if blocked.contains_key(service) {
                continue;
            }
            let Some(definition) = by_name.get(service.as_str()).copied() else {
                continue;
            };
            for dependency in hard_dependencies(definition) {
                if blocked.contains_key(&dependency.target) {
                    block_service(
                        blocked,
                        &definition.name,
                        BlockedReason::HardDependencyBlocked {
                            target: dependency.target,
                            kind: dependency.kind,
                        },
                    );
                    changed = true;
                    break;
                }
            }
        }
    }
}

fn dependency_eligible(mode: BootMode, definition: &ServiceDefinition) -> bool {
    mode == BootMode::Full
        || (definition.has_boot_trigger()
            && (definition.safe_mode || definition.error_control == ErrorControl::Critical))
}
