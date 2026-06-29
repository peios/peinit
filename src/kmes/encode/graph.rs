use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::execution::graph::{GraphExecutionEvent, GraphTerminalOutcome};

use super::super::payload::{finish_event, write_str_field, write_uint_field};

pub fn encode_graph_event(event: &GraphExecutionEvent) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(4);
    write_uint_field(&mut writer, "context_id", event.context_id.as_u64());
    write_str_field(&mut writer, "service", &event.service);
    write_str_field(&mut writer, "operation_id", &event.operation_id.to_string());
    write_str_field(
        &mut writer,
        "outcome",
        match event.outcome {
            GraphTerminalOutcome::Satisfied => "satisfied",
            GraphTerminalOutcome::Failed => "failed",
        },
    );
    finish_event("graph.operation_terminal", writer)
}
