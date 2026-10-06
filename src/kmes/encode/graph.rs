use crate::boundary::{BoundaryError, KmesEvent};
use crate::execution::graph::{GraphExecutionEvent, GraphTerminalOutcome};

use super::super::payload::Payload;
use super::super::types::GRAPH_OPERATION_ENDED;

/// `peinit.graph.operation.ended`: a member of a graph execution context
/// reached the outcome the graph counts.
pub fn encode_graph_event(event: &GraphExecutionEvent) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("graph.context", event.context_id.as_u64());
    payload.set("object.operation.guid", event.operation_id);
    payload.set("object.service.name", event.service.as_str());
    payload.set(
        "outcome.success",
        matches!(event.outcome, GraphTerminalOutcome::Satisfied),
    );
    payload.finish(GRAPH_OPERATION_ENDED)
}
