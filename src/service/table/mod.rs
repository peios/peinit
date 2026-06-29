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
