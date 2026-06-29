use crate::ids::{JobId, JobIdAllocator};

use super::model::{StartExecutionError, StartExecutionRequest};

pub(super) fn job_id_for_request(
    job_ids: &mut JobIdAllocator,
    request: &StartExecutionRequest,
) -> Result<JobId, StartExecutionError> {
    job_id_for_ready_start(
        job_ids,
        request.ready.reserved_job_id,
        request.started_at_ns,
    )
}

pub(super) fn job_id_for_ready_start(
    job_ids: &mut JobIdAllocator,
    reserved_job_id: Option<JobId>,
    started_at_ns: u64,
) -> Result<JobId, StartExecutionError> {
    if let Some(job_id) = reserved_job_id {
        return Ok(job_id);
    }
    job_ids
        .allocate_batch(1, started_at_ns)
        .map(|ids| ids[0])
        .map_err(StartExecutionError::JobIdAllocation)
}
