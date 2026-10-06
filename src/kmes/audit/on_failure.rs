use crate::boundary::{BoundaryError, KmesEvent};
use crate::supervisor::{
    SupervisorOnFailureLoopSuppressedDispatch, SupervisorOnFailureLoopSuppressionReason,
};

use crate::kmes::payload::Payload;
use crate::kmes::types::ON_FAILURE_SUPPRESSED;

/// `peinit.on-failure.suppressed`: an OnFailure handler peinit declined to
/// start, because starting it would have looped or gone too deep.
pub fn encode_on_failure_loop_suppressed_event(
    event: &SupervisorOnFailureLoopSuppressedDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.service.name", event.failed_service.as_str());
    payload.set(
        "object.service.on-failure.name",
        event.attempted_handler.as_str(),
    );
    payload.set("object.service.on-failure-chain", event.chain.as_slice());
    payload.set(
        "outcome.reason",
        on_failure_suppression_reason(event.reason),
    );
    payload.finish(ON_FAILURE_SUPPRESSED)
}

fn on_failure_suppression_reason(reason: SupervisorOnFailureLoopSuppressionReason) -> &'static str {
    match reason {
        SupervisorOnFailureLoopSuppressionReason::Cycle => "cycle",
        SupervisorOnFailureLoopSuppressionReason::MaxDepth { .. } => "max-depth",
    }
}
