use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::operation::store::{OperationEvent, OperationEventDetail};
use crate::operation::{OperationRecord, OperationSource, OperationType};

pub(super) fn explicit_stop_record_clears_fd_store(record: &OperationRecord) -> bool {
    record.operation_type == OperationType::Stop && record.source == OperationSource::Admin
}

pub(super) fn explicit_stop_event_clears_fd_store(event: &OperationEvent) -> bool {
    event.operation_type == OperationType::Stop
        && event.source == OperationSource::Admin
        && matches!(
            event.detail,
            OperationEventDetail::Completed { .. } | OperationEventDetail::Failed { .. }
        )
}

pub(super) fn synchronous_clear_fd_store_service(
    outcome: &LifecycleCommandOutcome,
) -> Option<&str> {
    let LifecycleCommandOutcome::SynchronousClear(clear) = outcome else {
        return None;
    };
    explicit_stop_event_clears_fd_store(&clear.completed)
        .then_some(clear.completed.service.as_str())
}
