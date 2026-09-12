mod activation;
mod model;
mod reload;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::service::definition::ServiceDefinition;
use crate::service::runtime::ServiceRuntimeSnapshot;

pub use model::{
    RestartBackoffDeadline, RestartWindowResetDeadline, ServiceActivationSnapshot, ServiceEntry,
    ServiceReloadSummary, ServiceTableError, ServiceTableTransition,
};

use reload::map_definitions;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ServiceTable {
    entries: BTreeMap<String, ServiceEntry>,
}

impl ServiceTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_boot_snapshot(
        definitions: Vec<ServiceDefinition>,
    ) -> Result<Self, ServiceTableError> {
        let mapped = map_definitions(definitions)?;
        let entries = mapped
            .into_iter()
            .map(|(name, definition)| {
                (
                    name.clone(),
                    ServiceEntry {
                        definition,
                        pending_definition: None,
                        runtime: ServiceRuntimeSnapshot::inactive(name),
                        definition_removed: false,
                    },
                )
            })
            .collect();
        Ok(Self { entries })
    }

    /// Enter a service the registry names but peinit could not decode, already
    /// Failed under `cause`.
    ///
    /// The entry is marked definition-removed: the placeholder definition is
    /// there so `status` can report the service, not so anything can start
    /// it. A reload that re-reads a repaired key restores the entry as it
    /// restores any definition-removed one; a `reset` clears it to Inactive,
    /// and Inactive does not retain a definition-removed entry, so the reset
    /// discards it. Inserted terminal rather than transitioned there, so that
    /// the discard rule does not fire on the way in (PEI-812).
    pub fn insert_undecodable_placeholder(
        &mut self,
        service: &str,
        cause: crate::service::runtime::TransitionCause,
        message: &str,
    ) -> Result<(), ServiceTableError> {
        let mut runtime = ServiceRuntimeSnapshot::inactive(service);
        runtime
            .transition(crate::service::runtime::ServiceTransition {
                to: crate::service::runtime::ServiceState::Failed,
                cause,
            })
            .map_err(ServiceTableError::Transition)?;
        self.entries.insert(
            service.to_string(),
            ServiceEntry {
                definition: ServiceDefinition::undecodable_placeholder(service, message),
                pending_definition: None,
                runtime,
                definition_removed: true,
            },
        );
        Ok(())
    }

    pub fn service_names(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }

    pub fn get(&self, service: &str) -> Option<&ServiceEntry> {
        self.entries.get(service)
    }

    pub fn definition(&self, service: &str) -> Option<&ServiceDefinition> {
        self.entries.get(service).map(|entry| &entry.definition)
    }

    pub fn runtime(&self, service: &str) -> Option<&ServiceRuntimeSnapshot> {
        self.entries.get(service).map(|entry| &entry.runtime)
    }

    pub(super) fn require_entry(&self, service: &str) -> Result<&ServiceEntry, ServiceTableError> {
        self.entries
            .get(service)
            .ok_or_else(|| ServiceTableError::UnknownService {
                service: service.to_string(),
            })
    }

    pub(super) fn require_entry_mut(
        &mut self,
        service: &str,
    ) -> Result<&mut ServiceEntry, ServiceTableError> {
        self.entries
            .get_mut(service)
            .ok_or_else(|| ServiceTableError::UnknownService {
                service: service.to_string(),
            })
    }
}
