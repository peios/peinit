use crate::boundary::ProcessController;
use crate::execution::start::{StartExecutionContext, timeout_service_main_start};

use super::super::relationships::apply_relationship_reactions_after_transitions;
use super::super::state::SupervisorError;
use super::super::work::SupervisorWork;
use super::model::{RunningServiceMainStartTimeout, RunningServiceMainStartTimeoutDispatch};

pub(in crate::supervisor::operation_maintenance) fn process_due_service_main_start_timeout(
    work: &mut SupervisorWork,
    controller: &mut dyn ProcessController,
    timeout: RunningServiceMainStartTimeout,
    now_ns: u64,
    max_parallel_starts: u32,
) -> Result<RunningServiceMainStartTimeoutDispatch, SupervisorError> {
    let job_id = timeout.job_id;
    let timeout = timeout_service_main_start(
        &mut StartExecutionContext {
            services: &mut work.services,
            operations: &mut work.operations,
            graph: &mut work.graph,
            jobs: &mut work.jobs,
            job_ids: &mut work.job_ids,
            start_store: &mut work.start,
            controller,
        },
        job_id,
        now_ns,
    )
    .map_err(SupervisorError::Start)?;
    work.pending_launches
        .retain(|pending_job_id| *pending_job_id != job_id);
    let mut start_dispatches = apply_relationship_reactions_after_transitions(
        work,
        &timeout.service_transitions,
        now_ns,
        max_parallel_starts,
    )?;
    start_dispatches.extend(work.release_after_graph_events(
        &timeout.graph_events,
        max_parallel_starts,
        now_ns,
    )?);

    Ok(RunningServiceMainStartTimeoutDispatch {
        timeout,
        start_dispatches,
    })
}
