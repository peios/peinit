use crate::service::runtime::{ServiceProgressReport, ServiceStoppingTimeoutEvidence};
use crate::submitted::{JobProgress, JobProgressUnit, PROGRESS_EVENT_INTERVAL_NS};

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

    /// Retain an accepted `PROGRESS=` value (PSPU §4.19). The unit is left
    /// as it was: each is replaced only by a datagram that carries it.
    pub fn update_progress(
        &mut self,
        service: &str,
        progress: JobProgress,
    ) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        entry.runtime.progress = Some(progress);
        Ok(())
    }

    /// Retain an accepted `PROGRESS_UNIT=` value (PSPU §4.19).
    pub fn update_progress_unit(
        &mut self,
        service: &str,
        unit: JobProgressUnit,
    ) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        entry.runtime.progress_unit = Some(unit);
        Ok(())
    }

    /// Whether a change of progress observed at `observed_at_ns` may become
    /// an event, at most once a second per incarnation (PSPU §4.A). When it
    /// may, the time is stamped and the retained progress returned; the
    /// retained value is the latest either way.
    pub fn take_progress_event(
        &mut self,
        service: &str,
        observed_at_ns: u64,
    ) -> Result<Option<ServiceProgressReport>, ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        let runtime = &mut entry.runtime;
        let due = runtime
            .last_progress_event_ns
            .is_none_or(|last| observed_at_ns.saturating_sub(last) >= PROGRESS_EVENT_INTERVAL_NS);
        if !due {
            return Ok(None);
        }
        runtime.last_progress_event_ns = Some(observed_at_ns);
        Ok(Some(ServiceProgressReport {
            progress: runtime.progress,
            unit: runtime.progress_unit,
        }))
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
