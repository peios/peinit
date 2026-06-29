use std::collections::{BTreeMap, BTreeSet};

use crate::service::runtime::ServiceState;
use crate::service::{ServiceTable, hard_dependencies};

use super::model::{
    ShutdownIgnoredService, ShutdownPlan, ShutdownPlanError, ShutdownStopParticipant,
    ShutdownStopWave,
};

pub fn plan_graceful_shutdown(services: &ServiceTable) -> Result<ShutdownPlan, ShutdownPlanError> {
    let mut completed_to_clear = Vec::new();
    let mut starting_to_kill = Vec::new();
    let mut stop_candidates = BTreeMap::new();
    let mut ignored = Vec::new();

    for service in services.service_names() {
        let runtime =
            services
                .runtime(service)
                .ok_or_else(|| ShutdownPlanError::MissingRuntime {
                    service: service.to_string(),
                })?;
        match runtime.state {
            ServiceState::Completed => completed_to_clear.push(service.to_string()),
            ServiceState::Starting => starting_to_kill.push(service.to_string()),
            ServiceState::Active | ServiceState::Reloading | ServiceState::Stopping => {
                stop_candidates.insert(
                    service.to_string(),
                    ShutdownStopParticipant {
                        service: service.to_string(),
                        state: runtime.state,
                        already_stopping: runtime.state == ServiceState::Stopping,
                    },
                );
            }
            ServiceState::Inactive
            | ServiceState::Backoff
            | ServiceState::Failed
            | ServiceState::Abandoned
            | ServiceState::Skipped => ignored.push(ShutdownIgnoredService {
                service: service.to_string(),
                state: runtime.state,
            }),
        }
    }

    Ok(ShutdownPlan {
        completed_to_clear,
        starting_to_kill,
        stop_waves: reverse_dependency_waves(services, stop_candidates)?,
        ignored,
    })
}

fn reverse_dependency_waves(
    services: &ServiceTable,
    mut remaining: BTreeMap<String, ShutdownStopParticipant>,
) -> Result<Vec<ShutdownStopWave>, ShutdownPlanError> {
    let mut waves = Vec::new();
    while !remaining.is_empty() {
        let dependents = dependents_by_target(services, remaining.keys())?;
        let ready = remaining
            .keys()
            .filter(|service| {
                dependents
                    .get(*service)
                    .is_none_or(|entries| entries.is_disjoint(&remaining_keys(&remaining)))
            })
            .cloned()
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return Err(ShutdownPlanError::DependencyCycle {
                services: remaining.keys().cloned().collect(),
            });
        }

        let mut wave = Vec::with_capacity(ready.len());
        for service in ready {
            let participant = remaining.remove(&service).ok_or_else(|| {
                ShutdownPlanError::MissingReadyService {
                    service: service.clone(),
                }
            })?;
            wave.push(participant);
        }
        waves.push(ShutdownStopWave { services: wave });
    }
    Ok(waves)
}

fn dependents_by_target<'a>(
    services: &ServiceTable,
    candidates: impl Iterator<Item = &'a String>,
) -> Result<BTreeMap<String, BTreeSet<String>>, ShutdownPlanError> {
    let candidates = candidates.cloned().collect::<BTreeSet<_>>();
    let mut dependents = BTreeMap::<String, BTreeSet<String>>::new();
    for dependent in &candidates {
        let definition =
            services
                .definition(dependent)
                .ok_or_else(|| ShutdownPlanError::MissingDefinition {
                    service: dependent.clone(),
                })?;
        for dependency in hard_dependencies(definition) {
            if candidates.contains(&dependency.target) {
                dependents
                    .entry(dependency.target)
                    .or_default()
                    .insert(dependent.clone());
            }
        }
    }
    Ok(dependents)
}

fn remaining_keys(remaining: &BTreeMap<String, ShutdownStopParticipant>) -> BTreeSet<String> {
    remaining.keys().cloned().collect()
}
