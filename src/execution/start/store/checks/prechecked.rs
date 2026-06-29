use crate::ids::OperationId;

use super::super::StartExecutionStore;
use super::model::PrecheckedGraphStart;

impl StartExecutionStore {
    pub fn record_prechecked_graph_start(&mut self, prechecked: PrecheckedGraphStart) {
        self.prechecked_graph_starts
            .insert(prechecked.ready.operation_id, prechecked);
    }

    pub fn remove_prechecked_graph_start(
        &mut self,
        operation_id: OperationId,
    ) -> Option<PrecheckedGraphStart> {
        self.prechecked_graph_starts.remove(&operation_id)
    }
}
