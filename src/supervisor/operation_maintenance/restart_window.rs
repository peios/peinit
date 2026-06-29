use crate::service::RestartWindowResetDeadline;

use super::super::state::SupervisorError;
use super::super::work::SupervisorWork;

pub(in crate::supervisor::operation_maintenance) fn reset_due_restart_windows(
    work: &mut SupervisorWork,
    due_resets: Vec<RestartWindowResetDeadline>,
    now_ns: u64,
) -> Result<Vec<RestartWindowResetDeadline>, SupervisorError> {
    let mut applied = Vec::new();
    for reset in due_resets {
        if !restart_window_reset_is_still_due(work, &reset, now_ns) {
            continue;
        }
        work.services
            .reset_restart_failures_after_window(&reset.service)
            .map_err(service_table_error)?;
        applied.push(reset);
    }
    Ok(applied)
}

fn restart_window_reset_is_still_due(
    work: &SupervisorWork,
    reset: &RestartWindowResetDeadline,
    now_ns: u64,
) -> bool {
    work.services
        .due_restart_window_resets(now_ns)
        .iter()
        .any(|current| current.service == reset.service && current.due_at_ns == reset.due_at_ns)
}

fn service_table_error(error: crate::service::ServiceTableError) -> SupervisorError {
    SupervisorError::Control(crate::execution::control::ControlExecutionError::ServiceTable(error))
}
