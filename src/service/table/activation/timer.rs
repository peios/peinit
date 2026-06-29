use super::super::ServiceTable;
use super::super::model::ServiceTableError;

impl ServiceTable {
    pub fn set_pending_timer(&mut self, service: &str) -> Result<bool, ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        let was_pending = entry.runtime.pending_timer;
        entry.runtime.pending_timer = true;
        Ok(!was_pending)
    }

    pub fn clear_pending_timer(&mut self, service: &str) -> Result<bool, ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        let was_pending = entry.runtime.pending_timer;
        entry.runtime.pending_timer = false;
        Ok(was_pending)
    }
}
