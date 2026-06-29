use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::operation::store::{OperationEvent, OperationEventDetail};

use super::super::labels::{operation_source_label, operation_state_label, operation_type_label};
use super::super::payload::{
    finish_event, write_optional_token_summary_field, write_str_field, write_uint_field,
};

pub fn encode_operation_event(event: &OperationEvent) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    match &event.detail {
        OperationEventDetail::Requested => {
            writer.write_map(6);
            write_operation_common(&mut writer, event);
            finish_event("operation.requested", writer)
        }
        OperationEventDetail::Started => {
            writer.write_map(6);
            write_operation_common(&mut writer, event);
            finish_event("operation.started", writer)
        }
        OperationEventDetail::Completed {
            duration_ns,
            result,
        } => {
            writer.write_map(8);
            write_operation_common(&mut writer, event);
            write_uint_field(&mut writer, "duration_ns", *duration_ns);
            write_str_field(&mut writer, "result", result);
            finish_event("operation.completed", writer)
        }
        OperationEventDetail::Failed {
            duration_ns,
            failure_reason,
        } => {
            writer.write_map(8);
            write_operation_common(&mut writer, event);
            write_uint_field(&mut writer, "duration_ns", *duration_ns);
            write_str_field(&mut writer, "failure_reason", failure_reason);
            finish_event("operation.failed", writer)
        }
        OperationEventDetail::Merged { merged_into } => {
            writer.write_map(7);
            write_operation_common(&mut writer, event);
            write_str_field(&mut writer, "merged_into", &merged_into.to_string());
            finish_event("operation.merged", writer)
        }
        OperationEventDetail::Cancelled { reason } => {
            writer.write_map(7);
            write_operation_common(&mut writer, event);
            write_str_field(&mut writer, "reason", reason);
            finish_event("operation.cancelled", writer)
        }
        OperationEventDetail::Aborted {
            duration_ns,
            reason,
        } => {
            writer.write_map(8);
            write_operation_common(&mut writer, event);
            write_uint_field(&mut writer, "duration_ns", *duration_ns);
            write_str_field(&mut writer, "reason", reason);
            finish_event("operation.aborted", writer)
        }
    }
}

fn write_operation_common(writer: &mut Writer, event: &OperationEvent) {
    write_str_field(writer, "operation_id", &event.operation_id.to_string());
    write_str_field(writer, "type", operation_type_label(event.operation_type));
    write_str_field(writer, "service", &event.service);
    write_str_field(writer, "source", operation_source_label(event.source));
    write_optional_token_summary_field(writer, "caller", event.caller.as_ref());
    write_str_field(writer, "state", operation_state_label(event.state));
}
