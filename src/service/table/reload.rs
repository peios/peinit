use std::collections::{BTreeMap, BTreeSet};

use crate::service::definition::ServiceDefinition;
use crate::service::runtime::ServiceRuntimeSnapshot;

use super::ServiceTable;
use super::model::{
    ServiceEntry, ServiceReloadSummary, ServiceTableError, retains_definition_after_removal,
};

impl ServiceTable {
    pub fn apply_definition_snapshot(
        &mut self,
        definitions: Vec<ServiceDefinition>,
    ) -> Result<ServiceReloadSummary, ServiceTableError> {
        let incoming = map_definitions(definitions)?;
        let mut summary = ServiceReloadSummary {
            added: Vec::new(),
            updated: Vec::new(),
            restored: Vec::new(),
            marked_removed: Vec::new(),
            discarded: Vec::new(),
        };

        for (name, definition) in &incoming {
            match self.entries.get_mut(name) {
                Some(entry) => {
                    if desired_definition(entry) != definition {
                        summary.updated.push(name.clone());
                    }
                    if entry.definition_removed {
                        summary.restored.push(name.clone());
                    }
                    apply_reloaded_definition(entry, definition);
                    entry.definition_removed = false;
                }
                None => {
                    self.entries.insert(
                        name.clone(),
                        ServiceEntry {
                            definition: definition.clone(),
                            pending_definition: None,
                            runtime: ServiceRuntimeSnapshot::inactive(name),
                            definition_removed: false,
                        },
                    );
                    summary.added.push(name.clone());
                }
            }
        }

        let incoming_names = incoming.keys().cloned().collect::<BTreeSet<_>>();
        let existing_names = self.entries.keys().cloned().collect::<Vec<_>>();
        for name in existing_names {
            if incoming_names.contains(&name) {
                continue;
            }
            let Some(entry) = self.entries.get_mut(&name) else {
                continue;
            };
            if retains_definition_after_removal(entry.runtime.state) {
                if entry.definition_removed {
                    continue;
                }
                entry.definition_removed = true;
                summary.marked_removed.push(name);
                continue;
            }
            self.entries.remove(&name);
            summary.discarded.push(name);
        }

        Ok(summary)
    }
}

fn desired_definition(entry: &ServiceEntry) -> &ServiceDefinition {
    entry
        .pending_definition
        .as_ref()
        .unwrap_or(&entry.definition)
}

fn apply_reloaded_definition(entry: &mut ServiceEntry, incoming: &ServiceDefinition) {
    if retains_definition_after_removal(entry.runtime.state) {
        let effective = runtime_effective_definition(&entry.definition, incoming);
        entry.pending_definition = if &effective == incoming {
            None
        } else {
            Some(incoming.clone())
        };
        entry.definition = effective;
    } else {
        entry.definition = incoming.clone();
        entry.pending_definition = None;
    }
}

fn runtime_effective_definition(
    current: &ServiceDefinition,
    incoming: &ServiceDefinition,
) -> ServiceDefinition {
    let mut effective = incoming.clone();

    effective.image_path = current.image_path.clone();
    effective.service_type = current.service_type;
    effective.identity = current.identity.clone();
    effective.required_privileges = current.required_privileges.clone();
    effective.error_control = current.error_control;
    effective.remain_after_exit = current.remain_after_exit;

    effective.requires = current.requires.clone();
    effective.wants = current.wants.clone();
    effective.binds_to = current.binds_to.clone();
    effective.conflicts = current.conflicts.clone();
    effective.on_failure = current.on_failure.clone();
    effective.conditions = current.conditions.clone();
    effective.asserts = current.asserts.clone();
    effective.triggers = current.triggers.clone();
    effective.disabled = current.disabled;

    effective
}

pub(super) fn map_definitions(
    definitions: Vec<ServiceDefinition>,
) -> Result<BTreeMap<String, ServiceDefinition>, ServiceTableError> {
    let mut mapped = BTreeMap::new();
    for definition in definitions {
        let name = definition.name.clone();
        if mapped.insert(name.clone(), definition).is_some() {
            return Err(ServiceTableError::DuplicateService { service: name });
        }
    }
    Ok(mapped)
}
