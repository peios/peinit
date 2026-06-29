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
