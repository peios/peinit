use std::collections::{BTreeMap, BTreeSet};

use crate::service::{ServiceDefinition, existing_start_order_dependency_targets};

pub(super) fn find_cycles(by_name: &BTreeMap<&str, &ServiceDefinition>) -> Vec<Vec<String>> {
    let mut search = CycleSearch::default();
    for service in by_name.keys() {
        search.visit(service, by_name);
    }
    search.cycles
}

#[derive(Default)]
struct CycleSearch {
    permanent: BTreeSet<String>,
    active: BTreeMap<String, usize>,
    stack: Vec<String>,
    seen: BTreeSet<Vec<String>>,
    cycles: Vec<Vec<String>>,
}

impl CycleSearch {
    fn visit(&mut self, service: &str, by_name: &BTreeMap<&str, &ServiceDefinition>) {
        if self.permanent.contains(service) {
            return;
        }
        if let Some(index) = self.active.get(service) {
            self.record_cycle(*index);
            return;
        }

        let Some(definition) = by_name.get(service) else {
            return;
        };
        self.active.insert(service.to_string(), self.stack.len());
        self.stack.push(service.to_string());
        for dependency in existing_start_order_dependency_targets(definition, |target| {
            by_name.contains_key(target)
        }) {
            self.visit(&dependency, by_name);
        }
        self.stack.pop();
        self.active.remove(service);
        self.permanent.insert(service.to_string());
    }

    fn record_cycle(&mut self, index: usize) {
        let cycle = self.stack[index..].to_vec();
        if self.seen.insert(canonical_cycle(&cycle)) {
            self.cycles.push(cycle);
        }
    }
}

fn canonical_cycle(cycle: &[String]) -> Vec<String> {
    let Some(first) = cycle.first() else {
        return Vec::new();
    };
    let mut best = cycle.to_vec();
    for index in 1..cycle.len() {
        if cycle[index] < *first || cycle[index..].iter().chain(&cycle[..index]).lt(best.iter()) {
            best = cycle[index..]
                .iter()
                .chain(&cycle[..index])
                .cloned()
                .collect();
        }
    }
    best
}
