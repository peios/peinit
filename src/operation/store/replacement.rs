use crate::ids::OperationId;
use crate::operation::conflict::OperationConflictDecision;

use super::{
    InsertPosition, OperationEvent, OperationRequest, OperationRequestOutcome, OperationStore,
    OperationStoreError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExistingTermination {
    Cancel(&'static str),
    Abort(&'static str),
}

impl OperationStore {
    pub(super) fn replace_existing(
        &mut self,
        request: OperationRequest,
        decision: OperationConflictDecision,
        existing_id: OperationId,
        termination: ExistingTermination,
        position: InsertPosition,
    ) -> Result<OperationRequestOutcome, OperationStoreError> {
        let terminated =
            self.terminate_existing(existing_id, request.created_at_ns, termination)?;
        let mut outcome = self.insert_requested(request, decision, position)?;
        outcome.events.insert(0, terminated);
        Ok(outcome)
    }

    fn terminate_existing(
        &mut self,
        id: OperationId,
        completed_at_ns: u64,
        termination: ExistingTermination,
    ) -> Result<OperationEvent, OperationStoreError> {
        let event = {
            let record = self
                .records
                .get_mut(&id)
                .ok_or(OperationStoreError::UnknownOperation { id })?;
            match termination {
                ExistingTermination::Cancel(reason) => {
                    record
                        .cancel(completed_at_ns, reason)
                        .map_err(OperationStoreError::Transition)?;
                    OperationEvent::cancelled(record)?
                }
                ExistingTermination::Abort(reason) => {
                    record
                        .abort(completed_at_ns, reason)
                        .map_err(OperationStoreError::Transition)?;
                    OperationEvent::aborted(record)?
                }
            }
        };
        self.remove_active(&event.service, id);
        Ok(event)
    }
}
