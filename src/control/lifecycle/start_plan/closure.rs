use std::collections::{BTreeMap, BTreeSet};

use crate::service::{ServiceDependencyKind, ServiceTable, hard_dependencies};

use super::model::{DependencyAvailability, StartBlockReason, StartPlanBlockedService};

pub(super) struct StartClosure {
    pub included: BTreeSet<String>,
    pub blocked: BTreeMap<String, StartPlanBlockedService>,
}

impl StartClosure {
    pub fn startable_services(&self) -> BTreeSet<String> {
        self.included
            .iter()
            .filter(|service| !self.blocked.contains_key(*service))
            .cloned()
            .collect()
    }
}

pub(super) fn collect_start_closure(services: &ServiceTable, service: &str) -> StartClosure {
    let mut collector = StartClosureCollector::new(services);
    collector.include_requested_service(service);
    collector.propagate_blocked();
    StartClosure {
        included: collector.included,
        blocked: collector.blocked,
    }
}

struct StartClosureCollector<'a> {
    services: &'a ServiceTable,
    included: BTreeSet<String>,
    blocked: BTreeMap<String, StartPlanBlockedService>,
}

impl<'a> StartClosureCollector<'a> {
    fn new(services: &'a ServiceTable) -> Self {
        Self {
            services,
            included: BTreeSet::new(),
            blocked: BTreeMap::new(),
        }
    }

    fn include_service(&mut self, service: &str) {
        self.include_service_with_mode(service, IncludeMode::Dependency);
    }

    fn include_requested_service(&mut self, service: &str) {
        self.include_service_with_mode(service, IncludeMode::Requested);
    }

    fn include_service_with_mode(&mut self, service: &str, mode: IncludeMode) {
        let Some(entry) = self.services.get(service) else {
            return;
        };
        if mode == IncludeMode::Dependency && entry.runtime.state.satisfies_dependents() {
            return;
        }
        if !self.included.insert(service.to_string()) {
            return;
        }

        for dependency in hard_dependencies(&entry.definition) {
            self.include_hard_dependency(service, &dependency.target, dependency.kind);
        }
        for target in &entry.definition.wants {
            self.include_wanted_dependency(target);
        }
    }

    fn include_hard_dependency(
        &mut self,
        service: &str,
        target: &str,
        kind: ServiceDependencyKind,
    ) {
        match dependency_entry(self.services, target) {
            DependencyEntry::Startable | DependencyEntry::Disabled => self.include_service(target),
            DependencyEntry::Satisfied => {}
            DependencyEntry::Missing => {
                self.block_unavailable(service, target, kind, DependencyAvailability::Missing)
            }
            DependencyEntry::DefinitionRemoved => self.block_unavailable(
                service,
                target,
                kind,
                DependencyAvailability::DefinitionRemoved,
            ),
        }
    }

    fn include_wanted_dependency(&mut self, target: &str) {
        if dependency_entry(self.services, target) == DependencyEntry::Startable {
            self.include_service(target);
        }
    }

    fn block_unavailable(
        &mut self,
        service: &str,
        target: &str,
        kind: ServiceDependencyKind,
        availability: DependencyAvailability,
    ) {
        self.blocked
            .entry(service.to_string())
            .or_insert(StartPlanBlockedService {
                service: service.to_string(),
                reason: StartBlockReason::HardDependencyUnavailable {
                    target: target.to_string(),
                    kind,
                    availability,
                },
            });
    }

    fn propagate_blocked(&mut self) {
        let mut changed = true;
        while changed {
            changed = false;
            for service in &self.included {
                if self.blocked.contains_key(service) {
                    continue;
                }
                let Some(entry) = self.services.get(service) else {
                    continue;
                };
                for dependency in hard_dependencies(&entry.definition) {
                    if self.blocked.contains_key(&dependency.target) {
                        self.blocked.insert(
                            service.clone(),
                            StartPlanBlockedService {
                                service: service.clone(),
                                reason: StartBlockReason::HardDependencyBlocked {
                                    target: dependency.target,
                                    kind: dependency.kind,
                                },
                            },
                        );
                        changed = true;
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncludeMode {
    Requested,
    Dependency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencyEntry {
    Startable,
    Satisfied,
    Disabled,
    Missing,
    DefinitionRemoved,
}

fn dependency_entry(services: &ServiceTable, service: &str) -> DependencyEntry {
    let Some(entry) = services.get(service) else {
        return DependencyEntry::Missing;
    };
    if entry.runtime.state.satisfies_dependents() {
        DependencyEntry::Satisfied
    } else if entry.definition_removed {
        DependencyEntry::DefinitionRemoved
    } else if entry.definition.disabled {
        DependencyEntry::Disabled
    } else {
        DependencyEntry::Startable
    }
}
