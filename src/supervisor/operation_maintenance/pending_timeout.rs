use crate::ids::OperationId;
use crate::operation::operation_timeout_result;
use crate::operation::store::OperationStoreError;

use super::super::state::SupervisorError;
use super::super::work::SupervisorWork;
use super::model::PendingOperationTimeout;

const OPERATION_TIMEOUT_DETAIL: &str = "operation maximum lifetime expired";
const DEPENDENCY_TIMEOUT_DETAIL: &str =
    "dependency operation timed out before graph start could complete";

pub(in crate::supervisor::operation_maintenance) fn fail_pending_operation_timeout(
    work: &mut SupervisorWork,
    operation_id: OperationId,
    now_ns: u64,
) -> Result<PendingOperationTimeout, SupervisorError> {
    let graph_events = work
        .graph
        .apply_operation_failed(operation_id)
        .map_err(SupervisorError::Graph)?;
    let mut operation_events = Vec::new();

    if graph_events.is_empty() {
        operation_events.push(
            work.operations
                .fail_operation(
                    operation_id,
                    now_ns,
                    operation_timeout_result(OPERATION_TIMEOUT_DETAIL),
                )
                .map_err(operation_store_error)?,
        );
    } else {
        for event in &graph_events {
            let detail = if event.operation_id == operation_id {
                OPERATION_TIMEOUT_DETAIL
            } else {
                DEPENDENCY_TIMEOUT_DETAIL
            };
            if work
                .operations
                .get(event.operation_id)
                .is_some_and(|operation| !operation.state.is_terminal())
            {
                operation_events.push(
                    work.operations
                        .fail_operation(
                            event.operation_id,
                            now_ns,
                            operation_timeout_result(detail),
                        )
                        .map_err(operation_store_error)?,
                );
            }
        }
    }

    Ok(PendingOperationTimeout {
        operation_events,
        graph_events,
    })
}

fn operation_store_error(error: OperationStoreError) -> SupervisorError {
    SupervisorError::Control(
        crate::execution::control::ControlExecutionError::OperationStore(error),
    )
}
