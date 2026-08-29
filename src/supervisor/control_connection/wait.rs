use crate::control::connection::{
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable,
    ControlConnectionWriteTurn, ControlConnectionWriteTurnError, ControlPendingWait,
};
use crate::control::query::QueryError;
use crate::control::wire::{
    ControlErrorCode, ControlResponseTimeProjection, control_error_response_line,
    control_lifecycle_ack_response_line_with_mode,
};
use crate::ids::OperationId;
use crate::operation::{OperationRecord, OperationType, is_operation_timeout};
use crate::supervisor::state::Supervisor;

impl Supervisor {
    pub fn flush_terminal_control_waits<I>(
        &self,
        connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
        observed_at_ns: u64,
        realtime_now_ns: u64,
    ) -> Result<SupervisorControlWaitFlushTurn, SupervisorControlWaitFlushError>
    where
        I: ControlConnectionIo,
    {
        let mut completed = Vec::new();
        for fd in connections.fds() {
            let Some(connection) = connections.get_mut(fd) else {
                continue;
            };
            let Some(pending) = connection.state().pending_wait().cloned() else {
                continue;
            };
            let (response_line, wait) = match pending {
                ControlPendingWait::Operation(wait) => {
                    if !self.wait_is_ready(wait.operation_id, observed_at_ns) {
                        continue;
                    }
                    let line = self
                        .control_wait_response_line(
                            wait.operation_id,
                            &wait.service,
                            observed_at_ns,
                        )
                        .map_err(SupervisorControlWaitFlushError::Response)?;
                    (
                        line,
                        SupervisorControlWaitCompletion::Operation(wait.operation_id),
                    )
                }
                ControlPendingWait::Job { job_id } => {
                    if !self.submitted_job_terminal(job_id) {
                        continue;
                    }
                    let line = self
                        .control_job_view_line(
                            job_id,
                            ControlResponseTimeProjection::new(observed_at_ns, realtime_now_ns),
                        )
                        .map_err(|error| {
                            SupervisorControlWaitFlushError::Response(
                                SupervisorControlWaitResponseError::Serialize(error.to_string()),
                            )
                        })?
                        .unwrap_or_default();
                    (line, SupervisorControlWaitCompletion::Job(job_id))
                }
            };
            connection.state_mut().clear_pending_wait();
            connection
                .state_mut()
                .enqueue_response(&response_line, false);
            let write = connection
                .flush()
                .map_err(SupervisorControlWaitFlushError::Write)?;
            if matches!(
                write,
                ControlConnectionWriteTurn::Complete { written, .. }
                    | ControlConnectionWriteTurn::Partial { written, .. } if written > 0
            ) {
                connection.state_mut().mark_activity(observed_at_ns);
            }
            completed.push(SupervisorControlWaitFlush {
                fd,
                completion: wait,
                write,
            });
        }

        Ok(SupervisorControlWaitFlushTurn { completed })
    }

    fn wait_is_ready(&self, operation_id: OperationId, observed_at_ns: u64) -> bool {
        self.operations.get(operation_id).is_none_or(|operation| {
            operation.state.is_terminal()
                || self.operation_timeout_expired(operation_id, observed_at_ns)
        })
    }

    fn control_wait_response_line(
        &self,
        operation_id: OperationId,
        service: &str,
        observed_at_ns: u64,
    ) -> Result<Vec<u8>, SupervisorControlWaitResponseError> {
        let Some(operation) = self.operations.get(operation_id) else {
            return control_error_response_line(
                ControlErrorCode::UnknownOperation,
                &format!("unknown operation {operation_id}"),
            )
            .map_err(SupervisorControlWaitResponseError::from);
        };

        if is_operation_timeout(operation)
            || self.operation_timeout_expired(operation_id, observed_at_ns)
        {
            return control_error_response_line(
                ControlErrorCode::OperationTimeout,
                &format!("operation {operation_id} timed out"),
            )
            .map_err(SupervisorControlWaitResponseError::from);
        }

        let view = self
            .service_status(service)
            .map_err(SupervisorControlWaitResponseError::Query)?;
        control_lifecycle_ack_response_line_with_mode(
            Some(operation_id),
            service,
            view.state,
            view.cause,
            &view.lifecycle_warnings,
            reload_mode(operation),
        )
        .map_err(SupervisorControlWaitResponseError::from)
    }
}

fn reload_mode(operation: &OperationRecord) -> Option<&'static str> {
    if operation.operation_type != OperationType::Reload {
        return None;
    }
    if operation.state == crate::operation::OperationState::Failed {
        return Some("failed");
    }
    let result = operation.result.as_deref()?;
    if result.contains("confirmed") {
        Some("confirmed")
    } else if result.contains("advisory") {
        Some("advisory")
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlWaitFlushTurn {
    pub completed: Vec<SupervisorControlWaitFlush>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlWaitFlush {
    pub fd: i32,
    pub completion: SupervisorControlWaitCompletion,
    pub write: ControlConnectionWriteTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorControlWaitCompletion {
    Operation(OperationId),
    Job(crate::ids::JobId),
}

#[derive(Debug)]
pub enum SupervisorControlWaitFlushError {
    Response(SupervisorControlWaitResponseError),
    Write(ControlConnectionWriteTurnError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorControlWaitResponseError {
    Query(QueryError),
    Serialize(String),
}

impl From<serde_json::Error> for SupervisorControlWaitResponseError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialize(error.to_string())
    }
}
