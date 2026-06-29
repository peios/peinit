use std::collections::BTreeMap;

use crate::service::{ServiceDefinition, all_declared_dependencies};

use super::model::{GraphContextBuildError, GraphDependency, GraphMember};

pub(super) fn definitions_by_name(
    definitions: &[ServiceDefinition],
) -> BTreeMap<&str, &ServiceDefinition> {
    definitions
        .iter()
        .map(|definition| (definition.name.as_str(), definition))
        .collect()
}

pub(super) fn context_dependencies<'a>(
    members: &BTreeMap<String, GraphMember>,
    definition: impl Fn(&str) -> Option<&'a ServiceDefinition>,
) -> Result<Vec<GraphDependency>, GraphContextBuildError> {
    let mut dependencies = Vec::new();

    for service in members.keys() {
        let definition = definition(service).ok_or_else(|| {
            GraphContextBuildError::MissingServiceDefinition {
                service: service.clone(),
            }
        })?;
        for dependency in all_declared_dependencies(definition) {
            if !members.contains_key(&dependency.target) {
                continue;
            }
            let dependency = GraphDependency {
                dependent: service.clone(),
                target: dependency.target,
                kind: dependency.kind,
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
    }

    Ok(dependencies)
}
