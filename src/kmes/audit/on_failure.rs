use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::supervisor::{
    SupervisorOnFailureLoopSuppressedDispatch, SupervisorOnFailureLoopSuppressionReason,
};

use crate::kmes::payload::{finish_event, write_str_field, write_string_array_field};

pub fn encode_on_failure_loop_suppressed_event(
    event: &SupervisorOnFailureLoopSuppressedDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(5);
    write_str_field(&mut writer, "failed_service", &event.failed_service);
    write_str_field(&mut writer, "attempted_handler", &event.attempted_handler);
    write_string_array_field(&mut writer, "chain", &event.chain);
    write_str_field(
        &mut writer,
        "reason",
        on_failure_suppression_reason(event.reason),
    );
    write_str_field(
        &mut writer,
        "message",
        &format!(
            "OnFailure loop suppressed after {} attempted {}",
            event.failed_service, event.attempted_handler
        ),
    );
    finish_event("on_failure.loop_suppressed", writer)
}

fn on_failure_suppression_reason(reason: SupervisorOnFailureLoopSuppressionReason) -> &'static str {
    match reason {
        SupervisorOnFailureLoopSuppressionReason::Cycle => "cycle",
        SupervisorOnFailureLoopSuppressionReason::MaxDepth { .. } => "max_depth",
    }
}
