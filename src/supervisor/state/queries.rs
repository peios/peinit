use std::collections::BTreeMap;

use crate::control::query::{
    OperationStatusView, QueryError, ServiceListItem, ServiceStatusView, ServiceTimerView,
    list_services, operation_status, service_status,
};
use crate::ids::OperationId;

use super::Supervisor;

impl Supervisor {
    pub fn service_status(&self, service: &str) -> Result<ServiceStatusView, QueryError> {
        let mut view = service_status(&self.services, &self.operations, &self.jobs, service)?;
        view.timers = self.calendar_timers.get(service).cloned().unwrap_or_default();
        Ok(view)
    }

    pub fn list_services(&self) -> Vec<ServiceListItem> {
        let mut items = list_services(&self.services);
        for item in &mut items {
            item.next_timer_ns = self
                .calendar_timers
                .get(&item.service)
                .and_then(|timers| timers.iter().filter_map(ServiceTimerView::fires_ns).min());
        }
        items
    }

    pub fn operation_status(
        &self,
        operation_id: OperationId,
    ) -> Result<OperationStatusView, QueryError> {
        operation_status(&self.operations, operation_id)
    }

    /// What the runtime has armed, replacing what it had before.
    pub fn set_calendar_timers(&mut self, timers: BTreeMap<String, Vec<ServiceTimerView>>) {
        self.calendar_timers = timers;
    }
}
