use std::collections::BTreeMap;

use crate::ids::OperationId;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct RestartStopLegStore {
    services: BTreeMap<OperationId, String>,
}

impl RestartStopLegStore {
    pub(super) fn record(&mut self, operation_id: OperationId, service: String) {
        self.services.insert(operation_id, service);
    }

    pub(super) fn take(&mut self, operation_id: OperationId) -> Option<String> {
        self.services.remove(&operation_id)
    }
}
