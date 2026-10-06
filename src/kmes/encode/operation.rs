use crate::boundary::{BoundaryError, KmesEvent};
use crate::operation::store::{OperationEvent, OperationEventDetail};

use super::super::labels::{operation_source_label, operation_state_label, operation_type_label};
use super::super::payload::{Payload, SUBJECT_TOKEN};
use super::super::types::{
    OPERATION_ENDED, OPERATION_MERGED, OPERATION_REQUESTED, OPERATION_STARTED,
};

/// The type an operation event is written as. The four ways an operation
/// can end other than merging are one type, told apart by
/// `object.operation.state` (PGSS §6.6).
pub fn operation_event_type(event: &OperationEvent) -> &'static str {
    match event.detail {
        OperationEventDetail::Requested => OPERATION_REQUESTED,
        OperationEventDetail::Started => OPERATION_STARTED,
        OperationEventDetail::Merged { .. } => OPERATION_MERGED,
        OperationEventDetail::Completed { .. }
        | OperationEventDetail::Failed { .. }
        | OperationEventDetail::Cancelled { .. }
        | OperationEventDetail::Aborted { .. } => OPERATION_ENDED,
    }
}

pub fn encode_operation_event(event: &OperationEvent) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.operation.guid", event.operation_id);
    payload.set(
        "object.operation.type",
        operation_type_label(event.operation_type),
    );
    payload.set("object.operation.source", operation_source_label(event.source));
    payload.set("object.operation.state", operation_state_label(event.state));
    payload.set("object.service.name", event.service.as_str());
    if let Some(caller) = &event.caller {
        payload.set_token(SUBJECT_TOKEN, caller);
    }
    match &event.detail {
        OperationEventDetail::Requested | OperationEventDetail::Started => {}
        OperationEventDetail::Completed {
            duration_ns,
            result,
        } => {
            ended(&mut payload, true, result, Some(*duration_ns));
        }
        OperationEventDetail::Failed {
            duration_ns,
            failure_reason,
        } => {
            ended(&mut payload, false, failure_reason, Some(*duration_ns));
        }
        OperationEventDetail::Cancelled { reason } => {
            ended(&mut payload, false, reason, None);
        }
        OperationEventDetail::Aborted {
            duration_ns,
            reason,
        } => {
            ended(&mut payload, false, reason, Some(*duration_ns));
        }
        OperationEventDetail::Merged { merged_into } => {
            payload.set("object.operation.merged-into.guid", *merged_into);
        }
    }
    payload.finish(operation_event_type(event))
}

fn ended(payload: &mut Payload, success: bool, detail: &str, duration_ns: Option<u64>) {
    payload.set("outcome.success", success);
    if !detail.is_empty() {
        payload.set("outcome.detail", detail);
    }
    payload.set_opt("object.operation.duration", duration_ns);
}
