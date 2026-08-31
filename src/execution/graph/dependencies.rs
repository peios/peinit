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
            // A level-less edge to a non-member is meaningless: the target
            // was either already satisfying dependents at plan time or is
            // outside this context, and there is no further condition to
            // wait for. A *level* edge is kept regardless of membership —
            // "netd is active" and "netd has published routed" are
            // different facts, and the second one has not been checked by
            // anything yet when the first was true at plan time.
            if !members.contains_key(&dependency.target) && dependency.level.is_none() {
                continue;
            }
            let dependency = GraphDependency {
                dependent: service.clone(),
                target: dependency.target,
                level: dependency.level,
                kind: dependency.kind,
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
    }

    Ok(dependencies)
}
