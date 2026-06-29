use std::collections::BTreeSet;

use crate::boot::BootMode;
use crate::boot::phase2::Phase2BootPlanError;
use crate::service::{ServiceDefinition, dependency_start_order};

use super::closure::collect_boot_closure;
use super::conflict::{critical_boot_conflict_exists, is_critical};
use super::model::ServiceMap;
use super::service::service_map;

pub(in crate::boot::phase2) fn safe_mode_required_for_full_boot(
    services: &[ServiceDefinition],
) -> Result<bool, Phase2BootPlanError> {
    let by_name = service_map(services)?;
    let closure = collect_boot_closure(BootMode::Full, services, &by_name);
    Ok(critical_boot_conflict_exists(&closure.included, &by_name)
        || critical_cycle_exists(&closure.included, &by_name))
}

fn critical_cycle_exists(included: &BTreeSet<String>, by_name: &ServiceMap<'_>) -> bool {
    let mut remaining = included.clone();
    while !remaining.is_empty() {
        match dependency_start_order(&remaining, |service| {
            by_name
                .get(service)
                .map(|definition| {
                    crate::service::start_order_dependency_targets(definition)
                        .into_iter()
                        .filter(|target| remaining.contains(target))
                        .collect()
                })
                .unwrap_or_default()
        }) {
            Ok(_) => return false,
            Err(cycle) => {
                if cycle.services.iter().any(|service| {
                    by_name
                        .get(service.as_str())
                        .is_some_and(|d| is_critical(d))
                }) {
                    return true;
                }
                for service in cycle.services {
                    remaining.remove(&service);
                }
            }
        }
    }
    false
}
