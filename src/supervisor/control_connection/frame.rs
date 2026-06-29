mod model;
mod shutdown;
mod standard;

pub use model::{
    SupervisorControlConnectionFrameTurn, SupervisorControlFrameContext,
    SupervisorControlFrameTurn, SupervisorControlFrameTurnError,
    SupervisorShutdownControlFrameContext,
};

use crate::control::wire::{
    ControlErrorCode, ControlFrameRejectReason, control_client_error_message,
    control_error_response_line,
};

fn frame_reject_response_line(
    reason: ControlFrameRejectReason,
) -> Result<Vec<u8>, serde_json::Error> {
    let code = ControlErrorCode::from(reason);
    control_error_response_line(code, control_client_error_message(code))
}
