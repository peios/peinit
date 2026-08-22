use std::collections::BTreeSet;

use crate::boot::BootMode;
use crate::boot::phase2::{Phase2BootPlanError, SafeModeDowngrade};
use crate::service::{ServiceDefinition, dependency_start_order};

use super::closure::collect_boot_closure;
use super::conflict::{critical_boot_conflicts, is_critical};
use super::model::ServiceMap;
use super::service::service_map;

/// Every finding that forces a Full boot down to Safe mode.
///
/// Returns the findings rather than a bare "yes it is required", because the
/// downgrade discards the Full-mode graph: the services in a critical cycle
/// are never entered into `blocked` and never marked Failed, so if the reason
/// is not captured here it is not captured anywhere. An operator would get a
/// Safe boot and no account of what caused it.
///
/// Per-service state is deliberately left alone. Safe mode was never going to
/// start those services, and marking them Failed would attach a state to a
/// service whose non-start had nothing to do with its own health — `status`
/// means "this service is broken", and it should keep meaning that.
pub(in crate::boot::phase2) fn safe_mode_downgrade_findings(
    services: &[ServiceDefinition],
) -> Result<Vec<SafeModeDowngrade>, Phase2BootPlanError> {
    let by_name = service_map(services)?;
    let closure = collect_boot_closure(BootMode::Full, services, &by_name);

    let mut findings = critical_boot_conflicts(&closure.included, &by_name)
        .into_iter()
        .map(|(service, target)| SafeModeDowngrade::CriticalBootConflict { service, target })
        .collect::<Vec<_>>();
    findings.extend(
        critical_cycles(&closure.included, &by_name)
            .into_iter()
            .map(|services| SafeModeDowngrade::CriticalCycle { services }),
    );
    Ok(findings)
}

/// Every cycle in the included set containing a Critical service.
///
/// Non-critical cycles are removed from the working set and the walk
/// continues, exactly as the previous existence check did — they do not force
/// Safe mode, and they are reported per service through the ordinary blocked
/// path instead.
fn critical_cycles(included: &BTreeSet<String>, by_name: &ServiceMap<'_>) -> Vec<Vec<String>> {
    let mut critical = Vec::new();
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
            Ok(_) => break,
            Err(cycle) => {
                if cycle.services.iter().any(|service| {
                    by_name
                        .get(service.as_str())
                        .is_some_and(|d| is_critical(d))
                }) {
                    critical.push(cycle.services.clone());
                }
                for service in cycle.services {
                    remaining.remove(&service);
                }
            }
        }
    }
    critical
}
