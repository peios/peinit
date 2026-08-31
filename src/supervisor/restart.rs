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

/// The abort reason §8.1 names, verbatim.
pub(super) const RESTART_DEFINITION_REMOVED: &str = "definition_removed_during_restart_stop_leg";
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
    dispatch: &mut ServiceMainJobTerminalDispatch,
    started_at_ns: u64,
) -> Result<Vec<RestartStartExecutionDispatch>, SupervisorError> {
    let Some(service) = dispatch.job_event.service.as_deref().map(ToString::to_string) else {
        return Ok(Vec::new());
    };
    let service = service.as_str();
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

    // §8.1: the definition went away while the stop leg was draining. The stop
    // still drained the instance -- that is what the transition above just
    // recorded -- but the start leg must not begin, because there is nothing
    // left to start from.
    //
    // `discarded_definition_removed` is the signal because the entry is
    // already gone by now: the transition that ended the stop leg is the same
    // one that discarded it. Without this the start leg looked the definition
    // up, found None, and raised MissingStartCredentials -- an internal error
    // out of the terminal-job path, which is not a control-flow outcome, so
    // the rest of that turn's work did not happen either (PEI-345).
    if dispatch
        .service_transitions
        .iter()
        .any(|transition| transition.discarded_definition_removed)
    {
        let event = work
            .operations
            .abort_operation(operation.id, started_at_ns, RESTART_DEFINITION_REMOVED)
            .map_err(|error| SupervisorError::Control(
                crate::execution::control::ControlExecutionError::OperationStore(error),
            ))?;
        dispatch.operation_events.push(event);
        return Ok(Vec::new());
    }

    let definition = work.services.definition(service).ok_or_else(|| {
        SupervisorError::MissingStartCredentials {
            service: service.to_string(),
        }
    })?;
    let resolved_identity = definition.identity.clone();
    let dispatch = begin_restart_start_leg(
        &mut work.services,
        &mut work.operations,
        &mut work.graph,
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
