use std::collections::{BTreeMap, BTreeSet};

use crate::boot::phase2::{BlockedReason, Phase2BootPlanError};
use crate::service::ServiceTrigger;
use crate::timer::calendar::CalendarSchedule;

use super::blocked::block_service;
use super::closure::propagate_blocked;
use super::model::{BlockedServiceDraft, ServiceMap, StartableSet};
use super::order::dependency_order;

pub(in crate::boot::phase2::graph) fn start_order_after_validation(
    included: &BTreeSet<String>,
    by_name: &ServiceMap<'_>,
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
) -> Vec<String> {
    block_invalid_timer_schedules(by_name, blocked);
    block_invalid_health_checks(included, by_name, blocked);
    block_dependency_cycles(included, by_name, blocked);
    loop {
        propagate_blocked(included, by_name, blocked);
        let startable = startable_services(included, blocked);
        match dependency_order(&startable, by_name) {
            Ok(order) => return order,
            Err(Phase2BootPlanError::Cycle { services }) => {
                for service in &services {
                    block_service(
                        blocked,
                        service,
                        BlockedReason::CycleDetected {
                            services: services.clone(),
                        },
                    );
                }
            }
            Err(_) => unreachable!("dependency_order only reports cycles"),
        }
    }
}

fn startable_services(
    included: &BTreeSet<String>,
    blocked: &BTreeMap<String, BlockedServiceDraft>,
) -> StartableSet {
    included
        .iter()
        .filter(|service| !blocked.contains_key(*service))
        .cloned()
        .collect()
}

fn block_dependency_cycles(
    included: &BTreeSet<String>,
    by_name: &ServiceMap<'_>,
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
) {
    let mut remaining = included.clone();
    while !remaining.is_empty() {
        match dependency_order(&remaining, by_name) {
            Ok(_) => return,
            Err(Phase2BootPlanError::Cycle { services }) => {
                for service in &services {
                    block_service(
                        blocked,
                        service,
                        BlockedReason::CycleDetected {
                            services: services.clone(),
                        },
                    );
                }
                for service in services {
                    remaining.remove(&service);
                }
            }
            Err(_) => unreachable!("dependency_order only reports cycles"),
        }
    }
}

fn block_invalid_health_checks(
    included: &BTreeSet<String>,
    by_name: &ServiceMap<'_>,
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
) {
    for service in included {
        let Some(definition) = by_name.get(service.as_str()).copied() else {
            continue;
        };
        if definition.health_check.is_some()
            && u64::from(definition.health_check_retries)
                .saturating_mul(definition.health_check_interval_secs)
                >= definition.restart_window_secs
        {
            block_service(
                blocked,
                service,
                BlockedReason::ValidationError {
                    message: format!(
                        "HealthCheck timing violates RestartWindow: retries {} * interval {} >= window {}",
                        definition.health_check_retries,
                        definition.health_check_interval_secs,
                        definition.restart_window_secs
                    ),
                },
            );
        }
    }
}

fn block_invalid_timer_schedules(
    by_name: &ServiceMap<'_>,
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
) {
    for definition in by_name.values() {
        for trigger in &definition.triggers {
            let ServiceTrigger::Timer { schedule } = trigger else {
                continue;
            };
            if let Err(error) = CalendarSchedule::parse(schedule) {
                block_service(
                    blocked,
                    &definition.name,
                    BlockedReason::ValidationError {
                        message: format!("Timer schedule {schedule:?} is invalid: {error}"),
                    },
                );
            }
        }
    }
}
