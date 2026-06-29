use std::collections::BTreeSet;

use super::ServiceDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceDependencyKind {
    Requires,
    Wants,
    BindsTo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDependency {
    pub target: String,
    pub kind: ServiceDependencyKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDependencyOrderCycle {
    pub services: Vec<String>,
}

pub fn hard_dependencies(definition: &ServiceDefinition) -> Vec<ServiceDependency> {
    definition
        .requires
        .iter()
        .cloned()
        .map(|target| ServiceDependency {
            target,
            kind: ServiceDependencyKind::Requires,
        })
        .chain(
            definition
                .binds_to
                .iter()
                .cloned()
                .map(|target| ServiceDependency {
                    target,
                    kind: ServiceDependencyKind::BindsTo,
                }),
        )
        .collect()
}

pub fn all_declared_dependencies(definition: &ServiceDefinition) -> Vec<ServiceDependency> {
    definition
        .requires
        .iter()
        .cloned()
        .map(|target| ServiceDependency {
            target,
            kind: ServiceDependencyKind::Requires,
        })
        .chain(
            definition
                .wants
                .iter()
                .cloned()
                .map(|target| ServiceDependency {
                    target,
                    kind: ServiceDependencyKind::Wants,
                }),
        )
        .chain(
            definition
                .binds_to
                .iter()
                .cloned()
                .map(|target| ServiceDependency {
                    target,
                    kind: ServiceDependencyKind::BindsTo,
                }),
        )
        .collect()
}

pub fn start_order_dependency_targets(definition: &ServiceDefinition) -> Vec<String> {
    definition
        .requires
        .iter()
        .chain(definition.binds_to.iter())
        .chain(definition.wants.iter())
        .cloned()
        .collect()
}

pub fn existing_start_order_dependency_targets(
    definition: &ServiceDefinition,
    contains: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    start_order_dependency_targets(definition)
        .into_iter()
        .filter(|target| contains(target))
        .filter(|target| seen.insert(target.clone()))
        .collect()
}

pub fn dependency_start_order(
    startable: &BTreeSet<String>,
    dependencies_for: impl Fn(&str) -> Vec<String>,
) -> Result<Vec<String>, ServiceDependencyOrderCycle> {
    let mut permanent = BTreeSet::new();
    let mut temporary = Vec::new();
    let mut order = Vec::new();

    for service in startable {
        visit_order(
            service,
            startable,
            &dependencies_for,
            &mut permanent,
            &mut temporary,
            &mut order,
        )?;
    }

    Ok(order)
}

fn visit_order(
    service: &str,
    startable: &BTreeSet<String>,
    dependencies_for: &impl Fn(&str) -> Vec<String>,
    permanent: &mut BTreeSet<String>,
    temporary: &mut Vec<String>,
    order: &mut Vec<String>,
) -> Result<(), ServiceDependencyOrderCycle> {
    if permanent.contains(service) {
        return Ok(());
    }
    if let Some(index) = temporary.iter().position(|entry| entry == service) {
        return Err(ServiceDependencyOrderCycle {
            services: temporary[index..].to_vec(),
        });
    }

    temporary.push(service.to_string());
    for dependency in dependencies_for(service) {
        if startable.contains(&dependency) {
            visit_order(
                &dependency,
                startable,
                dependencies_for,
                permanent,
                temporary,
                order,
            )?;
        }
    }
    temporary.pop();
    permanent.insert(service.to_string());
    order.push(service.to_string());
    Ok(())
}
