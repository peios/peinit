use super::super::ServiceTable;
use super::super::model::ServiceTableError;

impl ServiceTable {
    pub fn record_health_success(
        &mut self,
        service: &str,
        checked_at_ns: u64,
    ) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        entry.runtime.health.record_success(checked_at_ns);
        Ok(())
    }

    pub fn record_health_failure(
        &mut self,
        service: &str,
        checked_at_ns: u64,
    ) -> Result<u32, ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        Ok(entry.runtime.health.record_failure(checked_at_ns))
    }
}
