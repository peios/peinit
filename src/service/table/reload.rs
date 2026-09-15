use std::collections::{BTreeMap, BTreeSet};

use crate::boundary::UndecodableService;
use crate::service::definition::{ServiceDefinition, ServiceSecurityDescriptor};
use crate::service::runtime::{ServiceRuntimeSnapshot, TransitionCause};
use crate::service::synthesise_role_dependencies;

use super::ServiceTable;
use super::model::{
    ServiceEntry, ServiceReloadSummary, ServiceTableError, retains_definition_after_removal,
};

impl ServiceTable {
    pub fn apply_definition_snapshot(
        &mut self,
        definitions: Vec<ServiceDefinition>,
    ) -> Result<ServiceReloadSummary, ServiceTableError> {
        self.apply_definition_snapshot_with_undecodable(definitions, &[])
    }

    /// Apply a registry snapshot in which some keys exist but would not
    /// decode.
    ///
    /// Each undecodable key gets the treatment the boot planner gives it
    /// (§2.5): the service is Failed with `ValidationError` behind a
    /// placeholder definition, marked definition-removed so nothing can
    /// start it until a reload re-reads a repaired key. A service that is
    /// running when its key stops decoding cannot be failed — there is a
    /// process to supervise — so it is treated as a service whose definition
    /// was withdrawn: left running, marked definition-removed, discarded when
    /// it drains. Either way the name is reported in `undecodable`, not in
    /// `marked_removed` or `discarded`, because the key is still there
    /// (PEI-621).
    pub fn apply_definition_snapshot_with_undecodable(
        &mut self,
        definitions: Vec<ServiceDefinition>,
        undecodable: &[UndecodableService],
    ) -> Result<ServiceReloadSummary, ServiceTableError> {
        self.apply_definition_snapshot_with(definitions, undecodable, &[])
    }

    /// Apply a registry snapshot, leaving `frozen` entries' definitions as
    /// they are.
    ///
    /// For the boot window (§3.7): a boot-plan member whose launch has not
    /// been attempted must start from the plan's definition, so a change to
    /// it — a new definition, its removal, or a key that no longer decodes —
    /// is recorded rather than applied. A new definition goes to
    /// `pending_definition`, exactly as for a running service, and the entry
    /// is otherwise untouched; the name is listed under `deferred`. The
    /// reload after the window, with nothing frozen, applies it (PEI-350).
    pub fn apply_definition_snapshot_with(
        &mut self,
        definitions: Vec<ServiceDefinition>,
        undecodable: &[UndecodableService],
        frozen: &[String],
    ) -> Result<ServiceReloadSummary, ServiceTableError> {
        let incoming = map_definitions(definitions)?;
        let mut summary = ServiceReloadSummary {
            added: Vec::new(),
            updated: Vec::new(),
            restored: Vec::new(),
            marked_removed: Vec::new(),
            discarded: Vec::new(),
            undecodable: Vec::new(),
            deferred: Vec::new(),
        };
        let frozen = frozen.iter().map(String::as_str).collect::<BTreeSet<_>>();

        for (name, definition) in &incoming {
            match self.entries.get_mut(name) {
                Some(entry) if frozen.contains(name.as_str()) => {
                    if desired_definition(entry) != definition {
                        summary.deferred.push(name.clone());
                    }
                    entry.pending_definition = if &entry.definition == definition {
                        None
                    } else {
                        Some(definition.clone())
                    };
                }
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
        let undecodable_names = undecodable
            .iter()
            .map(|service| service.name.as_str())
            .collect::<BTreeSet<_>>();
        let existing_names = self.entries.keys().cloned().collect::<Vec<_>>();
        for name in existing_names {
            if incoming_names.contains(&name) || undecodable_names.contains(name.as_str()) {
                continue;
            }
            if frozen.contains(name.as_str()) {
                // Withdrawn during the boot window: the member still starts
                // from the plan, and the reload after the window removes it.
                summary.deferred.push(name);
                continue;
            }
            let Some(entry) = self.entries.get_mut(&name) else {
                continue;
            };
            // A compiled-in service is absent from every registry snapshot,
            // because the registry cannot define it — registryd bootstraps the
            // registry. Its absence therefore carries no information, and
            // reading it as a removal marks the one Critical service peinit
            // cannot afford to lose: `definition_removed` refuses restart AND
            // refuses Start/Restart/Reload from the control socket, so a later
            // registryd crash becomes unrecoverable, silently, because nothing
            // fails at the moment of the reload.
            //
            // The boot path has always guarded this by carrying the compiled-in
            // definition into the snapshot (`merge_phase2_service_table`). This
            // is the same rule stated where it belongs — on the data — so every
            // caller of a snapshot gets it rather than each remembering.
            if entry.definition.compiled_in {
                continue;
            }
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

        for service in undecodable {
            if incoming_names.contains(&service.name) {
                // The read cannot report a key as both; if it ever does, the
                // definition that decoded wins and the entry above stands.
                continue;
            }
            if frozen.contains(service.name.as_str()) && self.entries.contains_key(&service.name) {
                // The key stopped decoding during the boot window: the
                // member still starts from the plan, and the reload after
                // the window fails it (or restores it, if repaired by then).
                summary.deferred.push(service.name.clone());
                continue;
            }
            self.apply_undecodable(service)?;
            summary.undecodable.push(service.name.clone());
        }

        Ok(summary)
    }

    fn apply_undecodable(&mut self, service: &UndecodableService) -> Result<(), ServiceTableError> {
        if let Some(entry) = self.entries.get_mut(&service.name)
            && retains_definition_after_removal(entry.runtime.state)
        {
            entry.definition_removed = true;
            entry.pending_definition = None;
            return Ok(());
        }
        self.insert_undecodable_placeholder(
            &service.name,
            TransitionCause::ValidationError,
            &undecodable_message(service),
        )
    }
}

/// The description a placeholder carries: the same wording the boot planner
/// gives a blocked undecodable key, so `status` reads alike either way.
pub fn undecodable_message(service: &UndecodableService) -> String {
    format!("Service definition failed to decode: {}", service.message)
}

impl ServiceTable {
    /// The definition a registry snapshot should carry for a retained entry
    /// the registry does not define — the compiled-in registryd.
    ///
    /// Such a service has no `ServiceSecurity` of its own, so it takes the
    /// one on `Machine\System\Services` like any other service without one
    /// (§4.6), and drops back to the built-in default when the key carries
    /// none. Until this existed registryd kept its compiled-in default
    /// whatever the Services key said, so an administrator narrowing that
    /// key narrowed every service except the Critical one (PEI-1072).
    pub fn definition_inheriting_service_security(
        &self,
        service: &str,
        inherited: Option<&ServiceSecurityDescriptor>,
    ) -> Option<ServiceDefinition> {
        let mut definition = self.definition(service)?.clone();
        definition.service_security = inherited.cloned().unwrap_or_default();
        Some(definition)
    }

    /// The compiled-in services a snapshot does not name, each carrying the
    /// inherited descriptor, so a reload reaches them as it reaches every
    /// other service without a descriptor of its own (PEI-1072).
    pub fn compiled_in_definitions_absent_from(
        &self,
        snapshot: &[ServiceDefinition],
        inherited: Option<&ServiceSecurityDescriptor>,
    ) -> Vec<ServiceDefinition> {
        self.entries
            .iter()
            .filter(|(name, entry)| {
                entry.definition.compiled_in
                    && !snapshot.iter().any(|definition| &definition.name == *name)
            })
            .filter_map(|(name, _)| self.definition_inheriting_service_security(name, inherited))
            .collect()
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
    // Provenance is peinit's, not the registry's. Kept from `current` so a
    // registry entry that shadows a compiled-in name cannot strip the flag and
    // make the service removable on the reload after next.
    effective.compiled_in = current.compiled_in;

    effective
}

/// Index a definition set by name, deriving the dependencies its identities
/// imply on the way through.
///
/// The synthesis happens here rather than at either caller because this is
/// the one point both the boot table and a reload pass through, and an edge
/// that existed on one path and not the other would be worse than no edge at
/// all: the ordering would hold until the first reload and then silently stop.
pub(super) fn map_definitions(
    definitions: Vec<ServiceDefinition>,
) -> Result<BTreeMap<String, ServiceDefinition>, ServiceTableError> {
    let mut mapped = BTreeMap::new();
    for definition in synthesise_role_dependencies(definitions) {
        let name = definition.name.clone();
        if mapped.insert(name.clone(), definition).is_some() {
            return Err(ServiceTableError::DuplicateService { service: name });
        }
    }
    Ok(mapped)
}
