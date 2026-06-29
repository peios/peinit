use super::model::{ControlFrameDecision, ControlFrameRejectReason};

pub fn control_frame_decision(buffer: &[u8], max_request_bytes: usize) -> ControlFrameDecision {
    match buffer.iter().position(|byte| *byte == b'\n') {
        Some(newline_index) if newline_index > max_request_bytes => ControlFrameDecision::Reject {
            reason: ControlFrameRejectReason::RequestTooLarge,
        },
        Some(0) => ControlFrameDecision::Reject {
            reason: ControlFrameRejectReason::MalformedRequest,
        },
        Some(newline_index) => ControlFrameDecision::Complete {
            body: buffer[..newline_index].to_vec(),
            consumed: newline_index + 1,
        },
        None if buffer.len() > max_request_bytes => ControlFrameDecision::Reject {
            reason: ControlFrameRejectReason::RequestTooLarge,
        },
        None => ControlFrameDecision::Incomplete,
    }
}
