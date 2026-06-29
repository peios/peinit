use crate::execution::job_terminal::ServiceMainJobTerminalDispatch;
use crate::execution::restart_policy::{
    RestartPolicyRelaunchContext, RestartPolicyRelaunchRequest, begin_due_restart_policy_relaunch,
};
use crate::execution::start::{
    RestartStartExecutionDispatch, RestartStartExecutionOutcome, RestartStartExecutionRequest,
    begin_restart_start_leg,
};
use crate::operation::{OperationState, OperationType};
use crate::security::TokenSummary;

use super::dispatch::SupervisorRestartBackoffDispatch;
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

impl Supervisor {
    pub fn process_due_restart_backoffs(
        &mut self,
        now_ns: u64,
    ) -> Result<Vec<SupervisorRestartBackoffDispatch>, SupervisorError> {
        let due = self.services.due_restart_backoffs(now_ns);
        let mut work = SupervisorWork::from_supervisor(self);
        let mut dispatches = Vec::with_capacity(due.len());

        for deadline in due {
            let relaunch = begin_due_restart_policy_relaunch(
                &mut RestartPolicyRelaunchContext {
                    services: &mut work.services,
                    operations: &mut work.operations,
                    graph: &mut work.graph,
                    jobs: &mut work.jobs,
                    start_store: &mut work.start,
                    operation_ids: &mut work.operation_ids,
                    job_ids: &mut work.job_ids,
                },
                RestartPolicyRelaunchRequest {
                    service: deadline.service.clone(),
                    observed_at_ns: now_ns,
                    max_parallel_starts: self.settings.phase2.max_parallel_starts,
                },
            )
            .map_err(SupervisorError::RestartPolicy)?;
            work.queue_start_dispatches(&relaunch.start_dispatches);
            dispatches.push(SupervisorRestartBackoffDispatch {
                due: deadline,
                relaunch,
            });
        }

        work.commit(self);

        Ok(dispatches)
    }
}

pub(super) fn begin_restart_start_after_stop(
    work: &mut SupervisorWork,
    dispatch: &ServiceMainJobTerminalDispatch,
    started_at_ns: u64,
) -> Result<Vec<RestartStartExecutionDispatch>, SupervisorError> {
    let Some(service) = dispatch.job_event.service.as_deref() else {
        return Ok(Vec::new());
    };
    let Some(operation) = work.operations.current_for_service(service).cloned() else {
        return Ok(Vec::new());
    };
    if operation.operation_type != OperationType::Restart
        || operation.state != OperationState::Running
    {
        return Ok(Vec::new());
    }
    if work.control.take_restart_stop_leg(operation.id).is_none() {
        return Ok(Vec::new());
    }
    work.control.remove_stop_timeout(operation.id);

    let definition = work.services.definition(service).ok_or_else(|| {
        SupervisorError::MissingStartCredentials {
            service: service.to_string(),
        }
    })?;
    let resolved_identity = definition.identity.clone();
    let dispatch = begin_restart_start_leg(
        &mut work.services,
        &mut work.operations,
        &mut work.jobs,
        &mut work.job_ids,
        &mut work.start,
        RestartStartExecutionRequest {
            service: service.to_string(),
            operation_id: operation.id,
            resolved_identity: resolved_identity.clone(),
            token_summary: TokenSummary::requested_identity(resolved_identity),
            started_at_ns,
        },
    )
    .map_err(SupervisorError::Start)?;

    Ok(match dispatch {
        RestartStartExecutionOutcome::Job(dispatch) => vec![*dispatch],
        RestartStartExecutionOutcome::Terminal(_)
        | RestartStartExecutionOutcome::CheckPending(_) => Vec::new(),
    })
}
