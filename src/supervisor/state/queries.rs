use crate::control::query::{
    OperationStatusView, QueryError, ServiceListItem, ServiceStatusView, list_services,
    operation_status, service_status,
};
use crate::ids::OperationId;

use super::Supervisor;

impl Supervisor {
    pub fn service_status(&self, service: &str) -> Result<ServiceStatusView, QueryError> {
        service_status(&self.services, &self.operations, &self.jobs, service)
    }

    pub fn list_services(&self) -> Vec<ServiceListItem> {
        list_services(&self.services)
    }

    pub fn operation_status(
        &self,
        operation_id: OperationId,
    ) -> Result<OperationStatusView, QueryError> {
        operation_status(&self.operations, operation_id)
    }
}
