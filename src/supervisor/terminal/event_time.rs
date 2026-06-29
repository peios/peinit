use crate::execution::job_terminal::{ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError};
use crate::job::JobEventDetail;
use crate::supervisor::state::SupervisorError;

pub(super) fn terminal_event_time(
    dispatch: &ServiceMainJobTerminalDispatch,
) -> Result<u64, SupervisorError> {
    match dispatch.job_event.detail {
        JobEventDetail::Ended { ended_at_ns, .. } => Ok(ended_at_ns),
        _ => Err(SupervisorError::JobTerminal(
            ServiceMainJobTerminalError::NotTerminalEvent {
                job_id: dispatch.job_event.job_id,
            },
        )),
    }
}
