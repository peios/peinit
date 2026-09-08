use crate::service::runtime::ServiceStoppingTimeoutEvidence;

use super::super::ServiceTable;
use super::super::model::ServiceTableError;

impl ServiceTable {
    pub fn update_status_text(
        &mut self,
        service: &str,
        status_text: String,
    ) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        entry.runtime.status_text = Some(status_text);
        Ok(())
    }

    /// Record the level a service published, or clear it on an empty value.
    ///
    /// Returns whether it changed, so the caller only does the work of
    /// re-evaluating dependents when something actually moved — a daemon
    /// that republishes its level on a timer is a reasonable thing to
    /// write and must not cost a graph walk each time.
    pub fn update_level(&mut self, service: &str, level: &str) -> Result<bool, ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        let next = (!level.is_empty()).then(|| level.to_string());
        let changed = entry.runtime.level != next;
        entry.runtime.level = next;
        Ok(changed)
    }

    /// Forget a service's level, because it is no longer running.
    pub fn clear_level(&mut self, service: &str) -> Result<bool, ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        Ok(entry.runtime.level.take().is_some())
    }

    pub fn acknowledge_stopping(&mut self, service: &str) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        entry.runtime.stopping_acknowledged = true;
        Ok(())
    }

    pub fn record_stopping_timeout(
        &mut self,
        service: &str,
        evidence: ServiceStoppingTimeoutEvidence,
    ) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        entry.runtime.stopping_timeout = Some(evidence);
        Ok(())
    }
}
